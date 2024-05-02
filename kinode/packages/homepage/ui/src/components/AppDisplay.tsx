import classNames from "classnames"
import ColorDot from "./HexNum"
import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import { FaHeart, FaRegHeart } from "react-icons/fa6"
import { useState } from "react"

interface AppDisplayProps {
  app: HomepageApp
}

const AppDisplay: React.FC<AppDisplayProps> = ({ app }) => {
  const { favoriteApp, apps, setApps } = useHomepageStore()
  const [isHovered, setIsHovered] = useState(false)
  const onAppFavorited = async (appId: string) => {
    const res = await favoriteApp(appId.replace('/', ''))
    if (res.status === 201) {
      setApps(apps.map(a => a.package_name === app.package_name ? { ...app, is_favorite: !app.is_favorite } : a))
    } else {
      alert('Something went wrong. Please try again.')
    }
  }
  return <a
    className={classNames("flex-col-center gap-2 relative hover:opacity-90", { 'cursor-pointer': app.path })}
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
      className="absolute p-2 -top-2 -right-2 clear text-sm saturate-50"
      onClick={(e) => {
        e.preventDefault()
        onAppFavorited(app.path)
      }}
    >
      {app.is_favorite ? <FaHeart /> : <FaRegHeart />}
    </button>}
  </a>
}

export default AppDisplay

