/** FlowIsle 像素字模：严格 5×7 / 3×5 单格笔画，不缩放位图。 */

export const PIXEL_5X7: Record<string, string[]> = {
  '0': ['01110', '10001', '10011', '10101', '11001', '10001', '01110'],
  '1': ['00100', '01100', '00100', '00100', '00100', '00100', '01110'],
  '2': ['01110', '10001', '00001', '00010', '00100', '01000', '11111'],
  '3': ['11111', '00010', '00100', '00010', '00001', '10001', '01110'],
  '4': ['00010', '00110', '01010', '10010', '11111', '00010', '00010'],
  '5': ['11111', '10000', '11110', '00001', '00001', '10001', '01110'],
  '6': ['00110', '01000', '10000', '11110', '10001', '10001', '01110'],
  '7': ['11111', '00001', '00010', '00100', '01000', '01000', '01000'],
  '8': ['01110', '10001', '10001', '01110', '10001', '10001', '01110'],
  '9': ['01110', '10001', '10001', '01111', '00001', '00010', '01100'],
  ':': ['0', '0', '1', '0', '1', '0', '0'],
}

export const PIXEL_3X5: Record<string, string[]> = {
  '0': ['111', '101', '101', '101', '111'],
  '1': ['010', '110', '010', '010', '111'],
  '2': ['111', '001', '111', '100', '111'],
  '3': ['111', '001', '111', '001', '111'],
  '4': ['101', '101', '111', '001', '001'],
  '5': ['111', '100', '111', '001', '111'],
  '6': ['111', '100', '111', '101', '111'],
  '7': ['111', '001', '001', '001', '001'],
  '8': ['111', '101', '111', '101', '111'],
  '9': ['111', '101', '111', '001', '111'],
  ':': ['0', '1', '0', '1', '0'],
}

export interface PixelCell {
  x: number
  y: number
}

export interface PixelTextLayout {
  cells: PixelCell[]
  width: number
  height: number
}

/** 按整数单位生成矩形坐标；unit 变化时重新布局，不使用 transform:scale。 */
export function layoutPixelText(
  text: string,
  glyphs: Record<string, string[]>,
  unit: number,
  gap = 1,
): PixelTextLayout {
  let cursor = 0
  let height = 0
  const cells: PixelCell[] = []

  for (const char of text) {
    const glyph = glyphs[char]
    if (!glyph) {
      cursor += 3 + gap
      continue
    }
    const width = glyph[0].length
    height = Math.max(height, glyph.length)
    for (let y = 0; y < glyph.length; y += 1) {
      for (let x = 0; x < width; x += 1) {
        if (glyph[y][x] === '1') {
          cells.push({ x: (cursor + x) * unit, y: y * unit })
        }
      }
    }
    cursor += width + gap
  }

  return {
    cells,
    width: Math.max(0, cursor - gap) * unit,
    height: height * unit,
  }
}
