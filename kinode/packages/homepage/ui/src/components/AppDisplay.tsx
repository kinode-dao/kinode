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
    title={app?.label}
    style={!app?.path ? { pointerEvents: 'none', textDecoration: 'none !important', filter: 'grayscale(100%)' } : {}}
  >
    {app?.base64_icon
      ? <img className="app-icon" src={app.base64_icon} />
      : <img className="app-icon" src='/bird-orange.svg' />
    }
    <h6>{app?.label || app?.package_name}</h6>
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
      <span>{app?.favorite ? '★' : '☆'}</span>
    </button>}
  </a>
}

export default AppDisplay
