import classNames from "classnames"
import ColorDot from "./HexNum"
import { HomepageApp } from "../store/homepageStore"
import { FaHeart, FaRegHeart } from "react-icons/fa6"
import { useState } from "react"
import usePersistentStore from "../store/persistentStore"

interface AppDisplayProps {
  app: HomepageApp
}

const AppDisplay: React.FC<AppDisplayProps> = ({ app }) => {
  const { favoriteApp } = usePersistentStore();
  const [isHovered, setIsHovered] = useState(false)
  return <a
    className={classNames("flex-col-center gap-2 relative hover:opacity-90", { 'cursor-pointer': app.path, 'pointer-events-none': !app.path })}
    id={app.package_name}
    href={app.path}
    onMouseEnter={() => setIsHovered(true)}
    onMouseLeave={() => setIsHovered(false)}
  >
    {app.base64_icon
      ? <img
        src={app.base64_icon}
        className='h-32 w-32 rounded'
      />
      : <ColorDot num={app.state?.our_version || '0'} />}
    <h6>{app.label}</h6>
    {app.path && isHovered && <button
      className="absolute p-2 -top-2 -right-2 clear text-sm"
      onClick={(e) => {
        e.preventDefault()
        favoriteApp(app.package_name)
      }}
    >
      {app.is_favorite ? <FaHeart /> : <FaRegHeart />}
    </button>}
  </a>
}

export default AppDisplay

