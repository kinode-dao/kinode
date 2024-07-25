import React from 'react'
import { hexToRgb, hslToRgb, rgbToHex, rgbToHsl } from '../utils/colors'

interface ColorDotProps extends React.HTMLAttributes<HTMLSpanElement> {
  num: string,
  dotSize?: 'small' | 'medium' | 'large'
}

const ColorDot: React.FC<ColorDotProps> = ({
  num,
  dotSize = 'medium',
  ...props
}) => {
  num = num ? num : '';
  while (num.length < 6) {
    num = '0' + num
  }

  const leftHsl = rgbToHsl(hexToRgb(num.slice(0, 6)))
  const rightHsl = rgbToHsl(hexToRgb(num.length > 6 ? num.slice(num.length - 6) : num))
  leftHsl.s = rightHsl.s = 1
  const leftColor = rgbToHex(hslToRgb(leftHsl))
  const rightColor = rgbToHex(hslToRgb(rightHsl))

  const angle = (parseInt(num, 16) % 360) || -45

  const dotStyle = {
    '--left-color': leftColor,
    '--right-color': rightColor,
    '--gradient-angle': `${angle}deg`,
  } as React.CSSProperties;

  return (
    <div {...props} className={`color-dot-wrapper ${props.className || ''}`}>
      <div
        className={`color-dot color-dot-${dotSize}`}
        style={dotStyle}
      />
      {props.children}
    </div>
  )
}

export default ColorDot