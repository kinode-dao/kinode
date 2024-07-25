import { HomepageApp } from "../store/homepageStore"
import { useState } from "react"
import AppIconPlaceholder from "./AppIconPlaceholder"

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
      ? <img src={app.base64_icon} />
      : <AppIconPlaceholder
        text={app?.state?.our_version || '0'}
      />}
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

