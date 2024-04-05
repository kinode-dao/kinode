import { useState } from "react"
import classNames from 'classnames'
import { FaQuestion, FaX } from 'react-icons/fa6'

interface TooltipProps {
  text: string
  button?: React.ReactNode
  className?: string
  position?: "top" | "bottom" | "left" | "right"
}

export const Tooltip: React.FC<TooltipProps> = ({ text, button, className, position }) => {
  const [showTooltip, setShowTooltip] = useState(false)
  return <div className={classNames("flex place-items-center place-content-center text-sm relative cursor-pointer shrink", className)}>
    <div onClick={() => setShowTooltip(!showTooltip)}>
    {button || <button
        className="icon ml-4"
        type='button'
      >
        <FaQuestion />
      </button>}
    </div>
    <div className={classNames('absolute rounded bg-black p-2 min-w-[200px] z-10',
      {
        "hidden": !showTooltip,
        "top-8": position === "top" || !position,
        "bottom-8": position === "bottom",
        "right-8": position === "left",
        "left-8": position === "right",
      })}>
      {text}
    </div>
    <button type="button" className={classNames("absolute bg-black icon right-0 top-0", {
      "hidden": !showTooltip,
    })} onClick={() => setShowTooltip(false)}>
      <FaX />
    </button>
  </div>
}
