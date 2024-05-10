import classNames from "classnames";
import React from "react"
import { FaHome } from "react-icons/fa"
import { isMobileCheck } from "../utils/dimensions";

const HomeButton: React.FC = () => {
  const isMobile = isMobileCheck()
  return <button
    className={classNames("clear absolute p-2", {
      'top-2 left-2': isMobile,
      'top-8 left-8': !isMobile
    })}
    onClick={() => window.location.href = '/'}
  >
    <FaHome size={24} />
  </button>
}

export default HomeButton;