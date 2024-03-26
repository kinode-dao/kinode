import { useState } from "react"
import classNames from 'classnames'
import { FaQuestion } from 'react-icons/fa6'

interface TooltipProps {
  text: string
  button?: React.ReactNode
}

export const Tooltip: React.FC<TooltipProps> = ({ text, button }) => {
  const [showTooltip, setShowTooltip] = useState(false)
  return <div className="flex place-items-center place-content-center text-sm relative">
    <div onClick={() => setShowTooltip(!showTooltip)}>
      {button || <button
        className="icon ml-4"
        type='button'
      >
        <FaQuestion />
      </button>}
    </div>
    <div className={classNames('absolute top-16 rounded bg-black p-2 min-w-[200px] z-10', { "hidden": !showTooltip })}>
      {text}
    </div>
  </div>
}

