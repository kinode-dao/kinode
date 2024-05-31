import classNames from "classnames"
import { HomepageApp } from "../store/homepageStore"
import { FaHeart, FaRegHeart, } from "react-icons/fa6"
import { useState } from "react"
import usePersistentStore from "../store/persistentStore"
import { isMobileCheck } from "../utils/dimensions"
import AppIconPlaceholder from "./AppIconPlaceholder"

interface AppDisplayProps {
  app: HomepageApp
}

const AppDisplay: React.FC<AppDisplayProps> = ({ app }) => {
  const { favoriteApp, favoriteApps } = usePersistentStore();
  const [isHovered, setIsHovered] = useState(false)
  const isMobile = isMobileCheck()

  return <a
    className={classNames("flex-col-center gap-2 relative hover:opacity-90 transition-opacity", {
      'cursor-pointer': app.path,
      'cursor-not-allowed': !app.path,
    })}
    id={app.package_name}
    href={app.path}
    onMouseEnter={() => setIsHovered(true)}
    onMouseLeave={() => setIsHovered(false)}
  >
    {app.base64_icon
      ? <img
        src={app.base64_icon}
        className={classNames('rounded', {
          'h-8 w-8': isMobile,
          'h-16 w-16': !isMobile
        })}
      />
      : <AppIconPlaceholder
        text={app.state?.our_version || '0'}
        size={'small'}
        className="h-16 w-16"
      />}
    <h6>{app.label}</h6>
    {app.path && isHovered && <button
      className="absolute p-2 -top-2 -right-2 clear text-sm"
      onClick={(e) => {
        e.preventDefault()
        favoriteApp(app.package_name)
      }}
    >
      {favoriteApps[app.package_name]?.favorite ? <FaHeart /> : <FaRegHeart />}
    </button>}
  </a>
}

export default AppDisplay

