import { HomepageApp } from "../store/homepageStore"
import { useState } from "react"

interface AppDisplayProps {
  app?: HomepageApp
}

const AppDisplay: React.FC<AppDisplayProps> = ({ app }) => {
  const [isHovered, setIsHovered] = useState(false)

  return <a
    id={app?.package_name}
    href={app?.path}
    onMouseEnter={() => setIsHovered(true)}
    onMouseLeave={() => setIsHovered(false)}
  >
    {app?.base64_icon
      ? <img className="app-icon" src={app.base64_icon} />
      : <img className="app-icon" src='/bird-orange.svg' />
    }
    <h6>{app?.label || app?.package_name}</h6>
    {app?.path && isHovered && <button
      onClick={(e) => {
        e.preventDefault()
      }}
    >
    </button>}
  </a>
}

export default AppDisplay

