import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import { useState } from "react"

interface AppDisplayProps {
  app?: HomepageApp
}

const AppDisplay: React.FC<AppDisplayProps> = ({ app }) => {
  const { setApps } = useHomepageStore()
  const [isHovered, setIsHovered] = useState(false)

  return <a
    id={app?.package_name}
    href={app?.path || undefined}
    onMouseEnter={() => setIsHovered(true)}
    onMouseLeave={() => setIsHovered(false)}
    className="app-display"
    title={isHovered ? (app?.label || app?.package_name) : (!app?.path ? "This app does not serve a UI" : undefined)}
  >
    {app?.base64_icon
      ? <img className="app-icon" src={app.base64_icon} />
      : <img className="app-icon" src='/bird-orange.svg' />
    }
    <h6>{app?.label || app?.package_name}</h6>
    {isHovered && !app?.path && <p className="no-ui">This app does not serve a UI</p>}
    {app?.path && isHovered && <button className="app-fave-button"
      onClick={(e) => {
        e.preventDefault()
        fetch('/favorite', {
          method: 'POST',
          body: JSON.stringify([app?.id, !app?.favorite])
        }).then(() => {
          fetch('/apps', { credentials: 'include' }).then(res => res.json()).catch(() => [])
            .then(setApps)
        })
      }}
    >
      {app?.favorite ? '★' : '☆'}
    </button>}
  </a>
}

export default AppDisplay
