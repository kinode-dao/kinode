import { React, useState } from "react"
import { FaQuestion, FaX } from 'react-icons/fa6'

interface TooltipProps {
  text: string
  button?: React.ReactNode
  className?: string
  position?: "top" | "bottom" | "left" | "right"
}

export const Tooltip: React.FC<TooltipProps> = ({ text, button, className, position }) => {
  const [showTooltip, setShowTooltip] = useState(false)
  return <div className={`flex place-items-center place-content-center text-sm relative cursor-pointer shrink ${className}`}>
    <div onClick={() => setShowTooltip(!showTooltip)}>
      {button || <button
        className="icon ml-4"
        type='button'
      >
        <FaQuestion />
      </button>}
    </div>
    <div className={`absolute rounded bg-black p-2 min-w-[200px] z-10 ${position === "top" || !position ? "top-8" : position === "bottom" ? "bottom-8" : position === "left" ? "right-8" : "left-8"}`}>
      {text}
    </div>
    <button className={`absolute bg-black icon right-0 top-0 ${!showTooltip ? "hidden" : ""}`} onClick={() => setShowTooltip(false)}>
      <FaX />
    </button>
  </div >
}
