export const webInboxes: ReadonlyMap<string, string> = new Map([
  ['qq', 'https://mail.qq.com/'],
  ['163', 'https://mail.163.com/'],
  ['126', 'https://mail.126.com/'],
  ['gmail', 'https://mail.google.com/mail/#inbox'],
  ['outlook', 'https://outlook.live.com/mail/0/inbox'],
])

export function emailInboxUrl(provider?: string | null): string | null {
  const key = (provider ?? '').trim().toLowerCase()
  return webInboxes.get(key) ?? null
}
