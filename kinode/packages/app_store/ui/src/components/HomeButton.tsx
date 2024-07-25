import React from "react"
import { FaHome } from "react-icons/fa"

const HomeButton: React.FC = () => {
  return <button
    onClick={() => window.location.href = '/'}
  >
    <FaHome size={24} />
  </button>
}

export default HomeButton;