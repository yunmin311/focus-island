//! The notification watcher uses a small, read-only subset of IMAP over implicit TLS.
use imap_proto::{
    AttributeValue, Capability, Envelope, MailboxDatum, RequestId, Response, ResponseCode, Status,
};
use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const MAX_FRAME: usize = 256 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(20);
const ID_COMMAND: &str = concat!(
    "ID (\"name\" \"FlowIsle\" \"version\" \"",
    env!("CARGO_PKG_VERSION"),
    "\")"
);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Host,
    Dns,
    Connect,
    Tls,
    Transport,
    Protocol,
    Closed,
    InvalidCredential,
    Rejected,
    Authentication,
    Mailbox,
    Search,
}

impl Error {
    pub(crate) fn message(&self, provider: &str) -> &'static str {
        match self {
            Self::Host => "不支持的邮箱服务商，或自定义 IMAP 服务器地址无效",
            Self::Dns => "无法解析 IMAP 服务器，请检查服务器地址和网络",
            Self::Connect => "无法连接 IMAP 服务器（端口 993）",
            Self::Tls => "TLS 握手失败，请检查服务器证书、主机名和系统时间",
            Self::Transport => "IMAP 连接中断或超时，请检查网络",
            Self::Protocol => "IMAP 服务器返回了无法识别或格式错误的响应",
            Self::Closed => "IMAP 服务器关闭了连接，请稍后重试",
            Self::InvalidCredential => "邮箱地址或授权码含有不支持的控制字符",
            Self::Rejected => "IMAP 服务器拒绝了请求",
            Self::Mailbox => "无法打开 IMAP 收件箱",
            Self::Search => "无法搜索 IMAP 收件箱的未读邮件",
            Self::Authentication => match provider {
                "qq" => "QQ 邮箱登录失败，请确认已开启 IMAP，并检查客户端授权码是否正确或已失效",
                "163" | "126" => {
                    "网易邮箱登录失败，请确认已开启 IMAP，并检查客户端授权码是否正确或已失效"
                }
                "gmail" => "Gmail 登录失败，请检查应用专用密码是否正确、已撤销或受账号政策限制",
                "outlook" => "Outlook 登录失败，Outlook.com 要求 OAuth2，当前版本只支持授权码登录",
                _ => "IMAP 登录失败，请确认已开启 IMAP，并检查授权码是否正确或已失效",
            },
        }
    }
}

pub(crate) fn email_imap_host(provider: &str, custom_host: &str) -> Option<String> {
    let host = match provider {
        "qq" => "imap.qq.com",
        "163" => "imap.163.com",
        "126" => "imap.126.com",
        "gmail" => "imap.gmail.com",
        "outlook" => "outlook.office365.com",
        "custom" => custom_host.trim(),
        _ => return None,
    };
    let name = host.strip_suffix('.').unwrap_or(host);
    if name.len() > 253
        || !name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Mail {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
}

#[derive(Default)]
struct Reply {
    uid_validity: Option<u32>,
    unread: Option<Vec<u32>>,
    supports_idle: Option<bool>,
    mail: Vec<Mail>,
}

#[derive(Default)]
pub(crate) struct MailboxCursor {
    uid_validity: Option<u32>,
    seen: HashSet<u32>,
    max_uid: u32,
}

impl MailboxCursor {
    pub(crate) fn pending(&mut self, uid_validity: u32, unread: Vec<u32>) -> Vec<u32> {
        if self.uid_validity != Some(uid_validity) {
            self.uid_validity = Some(uid_validity);
            self.max_uid = unread.iter().copied().max().unwrap_or(0);
            self.seen = unread.into_iter().collect();
            return Vec::new();
        }

        // 只通知基线之后的新 UID。旧邮件被重新标成未读时 UID 仍小于基线，
        // 不应再次触发通知。
        let mut pending = Vec::new();
        for uid in unread {
            if uid > self.max_uid && self.seen.insert(uid) {
                self.max_uid = uid;
                pending.push(uid);
            }
        }
        pending
    }

    pub(crate) fn mark_seen(&mut self, uid: u32) {
        self.seen.insert(uid);
        self.max_uid = self.max_uid.max(uid);
    }
}

enum Frame<'a> {
    Id,
    Standard(Response<'a>),
}

pub(crate) struct Imap<S> {
    stream: BufReader<S>,
    next_tag: u64,
    frame: Vec<u8>,
}

impl<S: Read + Write> Imap<S> {
    pub(crate) fn new(stream: S) -> Result<Self, Error> {
        let mut session = Self {
            stream: BufReader::new(stream),
            next_tag: 0,
            frame: Vec::new(),
        };
        match session.read_frame(false).map_err(ReadError::public)? {
            Frame::Standard(Response::Data {
                status: Status::Ok, ..
            }) => Ok(session),
            Frame::Standard(Response::Data {
                status: Status::Bye,
                ..
            }) => Err(Error::Closed),
            _ => Err(Error::Protocol),
        }
    }

    pub(crate) fn open_inbox(
        &mut self,
        provider: &str,
        email: &str,
        secret: &str,
    ) -> Result<u32, Error> {
        self.command(&format!("LOGIN {} {}", quote(email)?, quote(secret)?))
            .map_err(|e| {
                if e == Error::Rejected {
                    Error::Authentication
                } else {
                    e
                }
            })?;
        if matches!(provider, "qq" | "163" | "126") {
            match self.command(ID_COMMAND) {
                Ok(_) | Err(Error::Rejected) => (),
                Err(error) => return Err(error),
            }
        }
        self.command("EXAMINE INBOX")
            .map_err(|e| {
                if e == Error::Rejected {
                    Error::Mailbox
                } else {
                    e
                }
            })?
            .uid_validity
            .ok_or(Error::Protocol)
    }

    pub(crate) fn supports_idle(&mut self) -> Result<bool, Error> {
        self.command("CAPABILITY")?
            .supports_idle
            .ok_or(Error::Protocol)
    }

    pub(crate) fn unread(&mut self) -> Result<Vec<u32>, Error> {
        let mut unread = self
            .command("UID SEARCH UNSEEN")
            .map_err(|e| {
                if e == Error::Rejected {
                    Error::Search
                } else {
                    e
                }
            })?
            .unread
            .ok_or(Error::Protocol)?;
        if unread.contains(&0) {
            return Err(Error::Protocol);
        }
        unread.sort_unstable();
        unread.dedup();
        Ok(unread)
    }

    pub(crate) fn fetch(&mut self, uids: &[u32]) -> Result<Vec<Mail>, Error> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut result = self
            .command(&format!("UID FETCH {set} (UID ENVELOPE)"))?
            .mail;
        result.retain(|mail| uids.contains(&mail.uid));
        Ok(result)
    }

    // IDLE uses the socket read timeout. On either timeout or a mailbox change,
    // send DONE and consume the matching completion before issuing another command.
    pub(crate) fn idle(&mut self) -> Result<bool, Error> {
        let tag = self.send("IDLE")?;
        loop {
            match self.read_frame(false).map_err(ReadError::public)? {
                Frame::Standard(Response::Continue { .. }) => break,
                Frame::Standard(Response::Done {
                    tag: received,
                    status: Status::Bad | Status::No,
                    ..
                }) if received.0 == tag => return Ok(false),
                Frame::Standard(response) => observe(response, &mut Reply::default())?,
                Frame::Id => return Err(Error::Protocol),
            }
        }
        match self.read_frame(false) {
            Ok(Frame::Standard(response)) => observe(response, &mut Reply::default())?,
            Err(ReadError::Timeout) => (),
            Ok(Frame::Id) => return Err(Error::Protocol),
            Err(error) => return Err(error.public()),
        }
        self.write(b"DONE\r\n")?;
        self.complete(&tag, false)?;
        Ok(true)
    }

    fn command(&mut self, command: &str) -> Result<Reply, Error> {
        let tag = self.send(command)?;
        self.complete(&tag, command == ID_COMMAND)
    }

    fn send(&mut self, command: &str) -> Result<String, Error> {
        self.next_tag = self.next_tag.checked_add(1).ok_or(Error::Protocol)?;
        let tag = format!("A{}", self.next_tag);
        self.write(format!("{tag} {command}\r\n").as_bytes())?;
        Ok(tag)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.stream
            .get_mut()
            .write_all(bytes)
            .map_err(|_| Error::Transport)?;
        self.stream.get_mut().flush().map_err(|_| Error::Transport)
    }

    fn complete(&mut self, tag: &str, allow_id: bool) -> Result<Reply, Error> {
        let mut reply = Reply::default();
        loop {
            match self.read_frame(allow_id).map_err(ReadError::public)? {
                Frame::Id => (),
                Frame::Standard(Response::Done {
                    tag: received,
                    status,
                    ..
                }) if received.0 == tag => {
                    return match status {
                        Status::Ok => Ok(reply),
                        Status::No | Status::Bad => Err(Error::Rejected),
                        _ => Err(Error::Protocol),
                    };
                }
                Frame::Standard(response) => observe(response, &mut reply)?,
            }
        }
    }

    fn read_frame(&mut self, allow_id: bool) -> Result<Frame<'_>, ReadError> {
        self.frame.clear();
        loop {
            let start = self.frame.len();
            let count = (&mut self.stream)
                .take((MAX_FRAME - start + 1) as u64)
                .read_until(b'\n', &mut self.frame)
                .map_err(|error| ReadError::io(error, self.frame.is_empty()))?;
            if count == 0 {
                return Err(ReadError::Public(Error::Closed));
            }
            let line = &self.frame[start..];
            if self.frame.len() > MAX_FRAME || !line.ends_with(b"\r\n") {
                return Err(Error::Protocol.into());
            }
            let line = &line[..line.len() - 2];
            let may_have_literal = start != 0 || is_data_response(line, b"ID") || is_fetch(line);
            let Some(size) = may_have_literal.then(|| literal_at_end(line)).flatten() else {
                break;
            };
            let size = size?;
            let end = self
                .frame
                .len()
                .checked_add(size)
                .filter(|end| *end <= MAX_FRAME)
                .ok_or(Error::Protocol)?;
            let start = self.frame.len();
            self.frame.resize(end, 0);
            self.stream
                .read_exact(&mut self.frame[start..])
                .map_err(|e| ReadError::io(e, false))?;
        }
        if is_data_response(&self.frame, b"ID") {
            if !allow_id || !valid_id(&self.frame[4..self.frame.len() - 2]) {
                return Err(Error::Protocol.into());
            }
            return Ok(Frame::Id);
        }
        // The pinned parser handles envelope grammar. Restrict FETCH attributes
        // before calling it: unused branches include a panic on INTERNALDATE NIL
        // and a recursive BODYSTRUCTURE grammar. METADATA can also panic on invalid UTF-8.
        if is_data_response(&self.frame, b"METADATA")
            || (is_fetch(&self.frame) && !valid_fetch_shape(&self.frame))
        {
            return Err(Error::Protocol.into());
        }
        if let Some(response) = parse_control_response(&self.frame) {
            return response.map(Frame::Standard).map_err(Into::into);
        }
        match imap_proto::parse_response(&self.frame) {
            Ok(([], response)) => Ok(Frame::Standard(response)),
            _ => Err(Error::Protocol.into()),
        }
    }
}

pub(crate) fn connect_imap(host: &str) -> Result<Imap<native_tls::TlsStream<TcpStream>>, Error> {
    let addresses = (host, 993).to_socket_addrs().map_err(|_| Error::Dns)?;
    let tls = native_tls::TlsConnector::builder()
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|_| Error::Tls)?;
    let mut error = Error::Connect;
    for address in addresses {
        match connect_tls(&tls, host, &address) {
            Ok(stream) => return Imap::new(stream),
            Err(Error::Connect) => continue,
            // Some 163 endpoints close the Schannel handshake when offered
            // TLS 1.3. Retry with TLS 1.2 and the same certificate validation.
            Err(Error::Tls) if host == "imap.163.com" => {
                let compatible = native_tls::TlsConnector::builder()
                    .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
                    .max_protocol_version(Some(native_tls::Protocol::Tlsv12))
                    .build()
                    .map_err(|_| Error::Tls)?;
                if let Ok(stream) = connect_tls(&compatible, host, &address) {
                    return Imap::new(stream);
                }
                error = Error::Tls;
            }
            Err(failure) => error = failure,
        }
    }
    Err(error)
}

fn connect_tls(
    tls: &native_tls::TlsConnector,
    host: &str,
    address: &std::net::SocketAddr,
) -> Result<native_tls::TlsStream<TcpStream>, Error> {
    let stream =
        TcpStream::connect_timeout(address, Duration::from_secs(8)).map_err(|_| Error::Connect)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|_| Error::Transport)?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|_| Error::Transport)?;
    tls.connect(host, stream).map_err(|_| Error::Tls)
}

// Status text is opaque and may contain private data. The pinned parser can
// slice it at a non-UTF-8 boundary after a response code, so parse its framing here.
fn parse_control_response(bytes: &[u8]) -> Option<Result<Response<'_>, Error>> {
    let line = bytes.strip_suffix(b"\r\n")?;
    if let Some(text) = line.strip_prefix(b"+") {
        return Some(if text.is_empty() || text.starts_with(b" ") {
            Ok(Response::Continue {
                code: None,
                information: None,
            })
        } else {
            Err(Error::Protocol)
        });
    }
    let mut words = line.splitn(3, |b| *b == b' ');
    let tag = words.next()?;
    let status = words.next()?;
    let status = [
        (b"OK".as_slice(), Status::Ok),
        (b"NO", Status::No),
        (b"BAD", Status::Bad),
        (b"BYE", Status::Bye),
        (b"PREAUTH", Status::PreAuth),
    ]
    .into_iter()
    .find_map(|(word, value)| status.eq_ignore_ascii_case(word).then_some(value))?;
    let text = words.next().unwrap_or_default();
    let code = match parse_status_code(text) {
        Ok(code) => code,
        Err(error) => return Some(Err(error)),
    };
    if tag == b"*" {
        return Some(Ok(Response::Data {
            status,
            code,
            information: None,
        }));
    }
    if tag.is_empty() || !tag.iter().all(u8::is_ascii_alphanumeric) {
        return Some(Err(Error::Protocol));
    }
    let tag = RequestId(String::from_utf8_lossy(tag).into_owned());
    Some(Ok(Response::Done {
        tag,
        status,
        code,
        information: None,
    }))
}

fn parse_status_code(text: &[u8]) -> Result<Option<ResponseCode<'_>>, Error> {
    let Some(code) = text.strip_prefix(b"[") else {
        return Ok(None);
    };
    let end = code
        .iter()
        .position(|byte| *byte == b']')
        .ok_or(Error::Protocol)?;
    if code.get(end + 1).is_some_and(|byte| *byte != b' ') {
        return Err(Error::Protocol);
    }
    let mut words = code[..end].splitn(2, |byte| *byte == b' ');
    if !words
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(b"UIDVALIDITY"))
    {
        return Ok(None);
    }
    let value = words
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.parse().ok())
        .filter(|value| *value != 0)
        .ok_or(Error::Protocol)?;
    Ok(Some(ResponseCode::UidValidity(value)))
}

fn observe(response: Response<'_>, reply: &mut Reply) -> Result<(), Error> {
    match response {
        Response::Data {
            status: Status::Ok,
            code: Some(ResponseCode::UidValidity(uid)),
            ..
        } if uid != 0 => reply.uid_validity = Some(uid),
        Response::Data {
            status: Status::Ok | Status::No,
            ..
        }
        | Response::MailboxData(
            MailboxDatum::Exists(_) | MailboxDatum::Recent(_) | MailboxDatum::Flags(_),
        )
        | Response::Expunge(_) => (),
        Response::Data {
            status: Status::Bye,
            ..
        } => return Err(Error::Closed),
        Response::Capabilities(caps) => {
            reply.supports_idle = Some(caps.iter().any(
                |cap| matches!(cap, Capability::Atom(atom) if atom.eq_ignore_ascii_case("IDLE")),
            ))
        }
        Response::IDs(ids) => reply.unread = Some(ids),
        Response::Fetch(_, attrs) => {
            let mut uid = None;
            let mut envelope = None;
            for attr in attrs {
                match attr {
                    AttributeValue::Uid(value) => uid = Some(value),
                    AttributeValue::Envelope(value) => envelope = Some(value),
                    _ => (),
                }
            }
            if let Some(envelope) = envelope {
                reply.mail.push(extract(
                    uid.filter(|uid| *uid != 0).ok_or(Error::Protocol)?,
                    &envelope,
                ));
            }
        }
        _ => return Err(Error::Protocol),
    }
    Ok(())
}

#[derive(Debug)]
enum ReadError {
    Timeout,
    Public(Error),
}

impl From<Error> for ReadError {
    fn from(error: Error) -> Self {
        Self::Public(error)
    }
}

impl ReadError {
    fn io(error: io::Error, empty: bool) -> Self {
        match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut if empty => Self::Timeout,
            io::ErrorKind::UnexpectedEof => Self::Public(Error::Closed),
            _ => Self::Public(Error::Transport),
        }
    }

    fn public(self) -> Error {
        match self {
            Self::Timeout => Error::Transport,
            Self::Public(error) => error,
        }
    }
}

fn quote(input: &str) -> Result<String, Error> {
    if input.chars().any(char::is_control) {
        return Err(Error::InvalidCredential);
    }
    Ok(format!(
        "\"{}\"",
        input.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn is_data_response(bytes: &[u8], kind: &[u8]) -> bool {
    bytes
        .strip_prefix(b"* ")
        .and_then(|rest| rest.split(|b| b.is_ascii_whitespace()).next())
        .is_some_and(|word| word.eq_ignore_ascii_case(kind))
}

fn is_fetch(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"* ") else {
        return false;
    };
    let mut words = rest.split(|b| *b == b' ');
    matches!((words.next(), words.next()), (Some(seq), Some(kind))
        if !seq.is_empty() && seq.iter().all(u8::is_ascii_digit) && kind.eq_ignore_ascii_case(b"FETCH"))
}

fn literal_at_end(line: &[u8]) -> Option<Result<usize, Error>> {
    let marker = line.iter().rposition(|byte| *byte == b'{')?;
    let digits = line.get(marker + 1..)?.strip_suffix(b"}")?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let before = marker;
    if before == 0 || !matches!(line[before - 1], b' ' | b'(') {
        return None;
    }
    Some(
        std::str::from_utf8(digits)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(Error::Protocol),
    )
}

fn take_string(input: &[u8]) -> Option<&[u8]> {
    match input.first()? {
        b'"' => {
            let mut escaped = false;
            for (i, byte) in input.iter().copied().enumerate().skip(1) {
                match (escaped, byte) {
                    (_, b'\0' | b'\r' | b'\n') => return None,
                    (true, b'"' | b'\\') => escaped = false,
                    (true, _) => return None,
                    (false, b'\\') => escaped = true,
                    (false, b'"') => return Some(&input[i + 1..]),
                    _ => (),
                }
            }
            None
        }
        b'{' => {
            let end = input.iter().position(|b| *b == b'}')?;
            let digits = std::str::from_utf8(&input[1..end]).ok()?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let size = digits.parse().ok()?;
            let literal = input.get(end + 1..)?.strip_prefix(b"\r\n")?;
            literal.get(size..)
        }
        _ => None,
    }
}

fn valid_id(input: &[u8]) -> bool {
    let Some(input) = input.strip_prefix(b" ") else {
        return false;
    };
    if input.eq_ignore_ascii_case(b"NIL") {
        return true;
    }
    let Some(mut rest) = input.strip_prefix(b"(") else {
        return false;
    };
    if rest == b")" {
        return true;
    }
    loop {
        let Some(value) = take_string(rest).and_then(|s| s.strip_prefix(b" ")) else {
            return false;
        };
        let Some(next) = take_string(value).or_else(|| {
            value
                .get(3..)
                .filter(|_| value[..3].eq_ignore_ascii_case(b"NIL"))
        }) else {
            return false;
        };
        if next == b")" {
            return true;
        }
        let Some(next) = next.strip_prefix(b" ") else {
            return false;
        };
        rest = next;
    }
}

fn valid_fetch_shape(bytes: &[u8]) -> bool {
    let Some(mut rest) = bytes
        .splitn(4, |byte| *byte == b' ')
        .nth(3)
        .and_then(|s| s.strip_prefix(b"("))
    else {
        return false;
    };
    let mut depth = 1;
    let mut expect_attribute = true;
    while let Some(&byte) = rest.first() {
        if expect_attribute {
            let Some(space) = rest.iter().position(|b| *b == b' ') else {
                return false;
            };
            if ![b"UID".as_slice(), b"ENVELOPE", b"FLAGS", b"MODSEQ"]
                .iter()
                .any(|name| rest[..space].eq_ignore_ascii_case(name))
            {
                return false;
            }
            expect_attribute = false;
            rest = &rest[space + 1..];
            continue;
        }
        match byte {
            b'"' | b'{' => {
                let Some(next) = take_string(rest) else {
                    return false;
                };
                rest = next;
                continue;
            }
            b'(' => {
                depth += 1;
                if depth > 16 {
                    return false;
                }
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[1..] == b"\r\n";
                }
            }
            b' ' if depth == 1 => expect_attribute = true,
            b'\r' | b'\n' => return false,
            _ => (),
        }
        rest = &rest[1..];
    }
    false
}

fn extract(uid: u32, envelope: &Envelope<'_>) -> Mail {
    let subject = envelope
        .subject
        .map(decode_mime_header)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "(无主题)".into());
    let from = envelope
        .from
        .as_ref()
        .and_then(|list| list.first())
        .and_then(|addr| {
            if let Some(name) = addr
                .name
                .map(decode_mime_header)
                .filter(|s| !s.trim().is_empty())
            {
                return Some(name);
            }
            Some(format!(
                "{}@{}",
                String::from_utf8_lossy(addr.mailbox?),
                String::from_utf8_lossy(addr.host?)
            ))
        })
        .unwrap_or_else(|| "未知发件人".into());
    let date = envelope
        .date
        .map(|date| String::from_utf8_lossy(date).trim().to_owned())
        .unwrap_or_default();
    Mail {
        uid,
        from,
        subject,
        date,
    }
}

fn decode_q_header(data: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(data.len());
    let mut bytes = data.iter().copied();
    while let Some(byte) = bytes.next() {
        match byte {
            b'_' => output.push(b' '),
            b'=' => {
                let high = (bytes.next()? as char).to_digit(16)?;
                let low = (bytes.next()? as char).to_digit(16)?;
                output.push((high * 16 + low) as u8);
            }
            b'!'..=b'~' if byte != b'?' => output.push(byte),
            _ => return None,
        }
    }
    Some(output)
}

fn decode_word(word: &str) -> Option<(String, usize)> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let (charset, rest) = word.strip_prefix("=?")?.split_once('?')?;
    let (encoding, rest) = rest.split_once('?')?;
    let (data, _) = rest.split_once("?=")?;
    let bytes = if encoding.eq_ignore_ascii_case("B") {
        STANDARD.decode(data).ok()?
    } else if encoding.eq_ignore_ascii_case("Q") {
        decode_q_header(data.as_bytes())?
    } else {
        return None;
    };
    let charset = charset.split('*').next()?;
    let charset = encoding_rs::Encoding::for_label(charset.as_bytes())?;
    let (decoded, _, _) = charset.decode(&bytes);
    let consumed = word.len() - rest.len() + data.len() + 2;
    Some((decoded.into_owned(), consumed))
}

pub(crate) fn decode_mime_header(raw: &[u8]) -> String {
    let input = String::from_utf8_lossy(raw);
    let mut output = String::new();
    let mut remainder = input.as_ref();
    let mut previous_word = false;
    while let Some(start) = remainder.find("=?") {
        let (between, token) = remainder.split_at(start);
        if let Some((decoded, consumed)) = decode_word(token) {
            if !previous_word
                || !between
                    .bytes()
                    .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            {
                output.push_str(between);
            }
            output.push_str(&decoded);
            remainder = &token[consumed..];
            previous_word = true;
        } else {
            output.push_str(between);
            output.push_str("=?");
            remainder = &token[2..];
            previous_word = false;
        }
    }
    output.push_str(remainder);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Replay<'a> {
        incoming: &'a [u8],
        written: Vec<u8>,
        timeout_when_empty: bool,
    }

    impl Read for Replay<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.incoming.is_empty() && self.timeout_when_empty {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.incoming.read(output)
        }
    }

    impl Write for Replay<'_> {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            self.written.write(input)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn replay(input: &[u8]) -> Imap<Replay<'_>> {
        Imap::new(Replay {
            incoming: input,
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn maps_fixed_hosts_and_accepts_only_custom_hostnames() {
        for (provider, host) in [
            ("qq", "imap.qq.com"),
            ("163", "imap.163.com"),
            ("126", "imap.126.com"),
            ("gmail", "imap.gmail.com"),
            ("outlook", "outlook.office365.com"),
            ("custom", "imap.example.invalid"),
        ] {
            assert_eq!(
                email_imap_host(provider, " Imap.Example.Invalid ").as_deref(),
                Some(host)
            );
        }
        for invalid in [
            "",
            "https://host",
            "host:993",
            "user@host",
            "-bad.host",
            "bad..host",
            "a b",
        ] {
            assert_eq!(email_imap_host("custom", invalid), None);
        }
        assert_eq!(email_imap_host("unknown", "imap.example.invalid"), None);
    }

    #[test]
    fn mime_words_keep_plain_spacing_and_malformed_input() {
        assert_eq!(
            decode_mime_header(b"Re: =?UTF-8?Q?=E4=B8=AD?= \t\r\n =?utf-8?B?5paH?= tail"),
            "Re: 中文 tail"
        );
        assert_eq!(decode_mime_header(b"=?gb18030?B?1tDOxA==?="), "中文");
        assert_eq!(
            decode_mime_header("=?UTF-8?Q?=中?= and =?UTF-8?Q?plain_text?=".as_bytes()),
            "=?UTF-8?Q?=中?= and plain text"
        );
        assert_eq!(
            decode_mime_header(b"=?unknown?B?YQ==?= and =?UTF-8?B?bad!"),
            "=?unknown?B?YQ==?= and =?UTF-8?B?bad!"
        );
    }

    #[test]
    fn only_valid_top_level_id_is_filtered_and_literals_keep_their_contents() {
        let mut client = replay(
            b"* OK ready\r\n* iD (\"name\" \"Coremail\" \"extension\" NIL)\r\nA1 OK done\r\n",
        );
        assert!(client.command(ID_COMMAND).is_ok());
        let mut client = replay(b"* OK ready\r\n* ID (broken)\r\nA1 OK done\r\n");
        assert_eq!(client.command(ID_COMMAND).err(), Some(Error::Protocol));
        let mut client = replay(b"* OK ready\r\n* ID NIL\r\nA1 OK done\r\n");
        assert_eq!(client.unread().err(), Some(Error::Protocol));
        let mut client = replay(b"* OK ready\r\n* 1 FETCH (UID 1 ENVELOPE (NIL {10}\r\n* ID NIL\r\n NIL NIL NIL NIL NIL NIL NIL NIL))\r\nA1 OK done\r\n");
        assert_eq!(client.fetch(&[1]).unwrap()[0].subject, "* ID NIL\r\n");
    }

    #[test]
    fn replay_recorded_163_capability_and_id_before_login() {
        let mut client = replay(include_bytes!("testdata/imap-163-public.imap"));
        assert!(!client.supports_idle().unwrap());
        assert!(client.command(ID_COMMAND).is_ok());
    }

    #[test]
    fn errors_are_typed_and_never_repeat_server_text() {
        for provider in ["qq", "163", "126", "gmail", "outlook", "custom"] {
            for status in ["NO", "BAD"] {
                let frame =
                    format!("* OK ready\r\nA1 {status} [AUTHENTICATIONFAILED] secret-marker\r\n");
                let error = replay(frame.as_bytes())
                    .open_inbox(provider, "test", "test")
                    .unwrap_err();
                assert_eq!(error, Error::Authentication);
                assert!(!error.message(provider).contains("secret-marker"));
            }
        }
        assert!(Error::Authentication.message("qq").contains("已失效"));
        assert!(Error::Authentication
            .message("gmail")
            .contains("应用专用密码"));
        assert!(Error::Authentication.message("outlook").contains("OAuth2"));
        assert!(Error::Tls.message("custom").contains("服务器证书"));
        assert_eq!(
            replay(b"* OK ready\r\nA1 NO unavailable\r\n").unread(),
            Err(Error::Search)
        );
        assert_eq!(
            replay(b"* OK ready\r\nA1 OK missing search data\r\n").unread(),
            Err(Error::Protocol)
        );
    }

    #[test]
    fn rejects_malformed_unknown_and_mismatched_responses() {
        for frame in [
            &b"Z9 OK wrong tag\r\n"[..],
            b"* UNKNOWN test\r\n",
            b"* 1 FETCH (INTERNALDATE NIL)\r\n",
            b"* METADATA INBOX (/shared/comment \"\xff\")\r\n",
            b"* SEARCH 42949672960\r\n",
            b"* 2 FETCH (UID 2 ENVELOPE (NIL {9999999}\r\n",
            b"* SEARCH 1\n",
            "* OK [UIDVALIDITY 1]中文\r\n".as_bytes(),
        ] {
            let input = [b"* OK ready\r\n", frame].concat();
            assert_eq!(replay(&input).unread(), Err(Error::Protocol));
        }
        assert_eq!(
            replay(b"* OK ready\r\n* BYE going away\r\n").unread(),
            Err(Error::Closed)
        );
        assert_eq!(
            replay(b"* OK ready\r\nA1 OK unexpectedly complete\r\n").idle(),
            Err(Error::Protocol)
        );
    }

    #[test]
    fn replay_idle_delivery_decodes_headers_and_suppresses_duplicate_uid() {
        let mut client = replay(include_bytes!("testdata/imap-idle.imap"));
        let mut cursor = MailboxCursor::default();
        let validity = client.open_inbox("qq", "fixture", "fake").unwrap();
        assert_eq!(validity, 987);
        assert!(client.supports_idle().unwrap());
        assert!(cursor
            .pending(validity, client.unread().unwrap())
            .is_empty());
        assert!(client.idle().unwrap());
        let pending = cursor.pending(validity, client.unread().unwrap());
        assert_eq!(pending, [43]);
        let mail = client.fetch(&pending).unwrap();
        assert_eq!(
            (mail[0].uid, mail[0].from.as_str(), mail[0].subject.as_str()),
            (43, "测试", "中文")
        );
        cursor.mark_seen(mail[0].uid);
        assert!(cursor
            .pending(validity, client.unread().unwrap())
            .is_empty());
        let written = String::from_utf8(client.stream.into_inner().written).unwrap();
        assert!(written.contains("A6 IDLE\r\nDONE\r\nA7 UID SEARCH UNSEEN"));
        assert!(written.contains("A8 UID FETCH 43 (UID ENVELOPE)"));
    }

    #[test]
    fn ignores_old_uid_marked_unread_after_baseline() {
        let mut cursor = MailboxCursor::default();
        assert!(cursor.pending(1, vec![10, 20]).is_empty());
        assert!(cursor.pending(1, vec![10, 20, 15]).is_empty());
        assert_eq!(cursor.pending(1, vec![10, 20, 21]), vec![21]);
        assert!(cursor.pending(1, vec![10, 15, 20, 21]).is_empty());
    }

    #[test]
    fn replay_poll_delivery_and_explicit_idle_refusal() {
        let mut client = replay(include_bytes!("testdata/imap-poll.imap"));
        let mut cursor = MailboxCursor::default();
        let validity = client.open_inbox("163", "fixture", "fake").unwrap();
        assert!(!client.supports_idle().unwrap());
        assert!(cursor
            .pending(validity, client.unread().unwrap())
            .is_empty());
        let pending = cursor.pending(validity, client.unread().unwrap());
        assert_eq!(client.fetch(&pending).unwrap()[0].subject, "poll fixture");
        assert!(cursor.pending(validity + 1, vec![1]).is_empty());
        let mut client =
            replay(b"* OK ready\r\nA1 BAD unsupported\r\n* SEARCH 1\r\nA2 OK done\r\n");
        assert!(!client.idle().unwrap());
        assert_eq!(client.unread().unwrap(), [1]);
    }
}
