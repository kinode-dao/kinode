import classNames from 'classnames'
import React from 'react'
import { hexToRgb, hslToRgb, rgbToHex, rgbToHsl } from '../utils/colors'

interface ColorDotProps extends React.HTMLAttributes<HTMLSpanElement> {
  num: string,
}

const ColorDot: React.FC<ColorDotProps> = ({
  num,
  ...props
}) => {
  num = (num || '').replace(/(0x|\.)/g, '')

  while (num.length < 6) {
    num = '0' + num
  }

  const leftHsl = rgbToHsl(hexToRgb(num.slice(0, 6)))
  const rightHsl = rgbToHsl(hexToRgb(num.length > 6 ? num.slice(num.length - 6) : num))
  leftHsl.s = rightHsl.s = 1
  const leftColor = rgbToHex(hslToRgb(leftHsl))
  const rightColor = rgbToHex(hslToRgb(rightHsl))

  const angle = (parseInt(num, 16) % 360) || -45

  return (
    <div {...props} className={classNames('flex', props.className)}>
      <div
        className='m-0 align-self-center h-32 w-32 border border-8 rounded-full outline-black'
        style={{
          borderTopColor: leftColor,
          borderRightColor: rightColor,
          borderBottomColor: rightColor,
          borderLeftColor: leftColor,
          background: `linear-gradient(${angle}deg, ${leftColor} 0 50%, ${rightColor} 50% 100%)`,
          filter: 'saturate(0.25)'
        }} />
      {props.children}
    </div>
  )
}

export default ColorDot
