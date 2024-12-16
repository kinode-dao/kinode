export type RgbType = { r: number, g: number, b: number }
export type HslType = { h: number, s: number, l: number }

export const rgbToHsl: (rgb: RgbType) => HslType = ({ r, g, b }) => {
  r /= 255; g /= 255; b /= 255;
  let max = Math.max(r, g, b);
  let min = Math.min(r, g, b);
  let d = max - min;
  let h = 0;
  if (d === 0) h = 0;
  else if (max === r) h = ((((g - b) / d) % 6) + 6) % 6; // the javascript modulo operator handles negative numbers differently than most other languages
  else if (max === g) h = (b - r) / d + 2;
  else if (max === b) h = (r - g) / d + 4;
  let l = (min + max) / 2;
  let s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  return { h: h * 60, s, l };
}

export const hslToRgb: (hsl: HslType) => RgbType = ({ h, s, l }) => {
  let c = (1 - Math.abs(2 * l - 1)) * s;
  let hp = h / 60.0;
  let x = c * (1 - Math.abs((hp % 2) - 1));
  let rgb1 = [0, 0, 0];
  if (isNaN(h)) rgb1 = [0, 0, 0];
  else if (hp <= 1) rgb1 = [c, x, 0];
  else if (hp <= 2) rgb1 = [x, c, 0];
  else if (hp <= 3) rgb1 = [0, c, x];
  else if (hp <= 4) rgb1 = [0, x, c];
  else if (hp <= 5) rgb1 = [x, 0, c];
  else if (hp <= 6) rgb1 = [c, 0, x];
  let m = l - c * 0.5;
  return {
    r: Math.round(255 * (rgb1[0] + m)),
    g: Math.round(255 * (rgb1[1] + m)),
    b: Math.round(255 * (rgb1[2] + m))
  };
}

export const hexToRgb: (hex: string) => RgbType = (hex) => {
  var result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  return result ? {
    r: parseInt(result[1], 16),
    g: parseInt(result[2], 16),
    b: parseInt(result[3], 16)
  } : { r: 0, g: 0, b: 0 };
}

export const rgbToHex: (rgb: RgbType) => string = ({ r, g, b }) => {
  return '#' + ((1 << 24) + (r << 16) + (g << 8) + b).toString(16).slice(1);
}

export const generateDistinguishableColors = (numColors: number, mod?: number, sat?: number, val?: number) => {
  const colors: string[] = [];
  const saturation = sat || 0.75;
  const value = val || 0.75;

  for (let i = 0; i < numColors; i++) {
    const hue = i / numColors * (+(mod || 1) || 1);
    const [r, g, b] = hsvToRgb(hue, saturation, value);
    const hexColor = rgbToHex2(r, g, b);
    colors.push(hexColor);
  }

  return colors;
}

export const hsvToRgb = (h: number, s: number, v: number) => {
  let [r, g, b] = [0, 0, 0];
  const i = Math.floor(h * 6);
  const f = h * 6 - i;
  const p = v * (1 - s);
  const q = v * (1 - f * s);
  const t = v * (1 - (1 - f) * s);

  switch (i % 6) {
    case 0:
      (r = v), (g = t), (b = p); // eslint-disable-line
      break;
    case 1:
      (r = q), (g = v), (b = p); // eslint-disable-line
      break;
    case 2:
      (r = p), (g = v), (b = t); // eslint-disable-line
      break;
    case 3:
      (r = p), (g = q), (b = v); // eslint-disable-line
      break;
    case 4:
      (r = t), (g = p), (b = v); // eslint-disable-line
      break;
    case 5:
      (r = v), (g = p), (b = q); // eslint-disable-line
      break;
  }

  return [Math.round(r * 255), Math.round(g * 255), Math.round(b * 255)];
}

export const componentToHex = (c: number) => {
  const hex = c.toString(16);
  return hex.length === 1 ? '0' + hex : hex;
}

export const rgbToHex2 = (r: number, g: number, b: number) => {
  return '#' + componentToHex(r) + componentToHex(g) + componentToHex(b);
}