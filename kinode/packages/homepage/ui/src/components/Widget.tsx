import classNames from "classnames"
import { FaEye, FaEyeSlash } from "react-icons/fa6"
import { useState } from "react"
import usePersistentStore from "../store/persistentStore"
import useHomepageStore from "../store/homepageStore"
import { isMobileCheck } from "../utils/dimensions"

interface WidgetProps {
  package_name: string,
  widget: string
  forceLarge?: boolean
}

const Widget: React.FC<WidgetProps> = ({ package_name, widget, forceLarge }) => {
  const { apps } = useHomepageStore()
  const { widgetSettings, toggleWidgetVisibility } = usePersistentStore()
  const [isHovered, setIsHovered] = useState(false)
  const isMobile = isMobileCheck()
  const isLarge = forceLarge || widgetSettings[package_name]?.size === "large"
  const isSmall = !widgetSettings[package_name]?.size || widgetSettings[package_name]?.size === "small"
  return <div
    className={classNames("self-stretch flex-col-center shadow-lg rounded-lg relative", {
      "max-w-1/2 min-w-1/2": isLarge && !isMobile,
      "max-w-1/4 min-w-1/4": isSmall && !isMobile,
      'w-full': isMobile
    })}
    onMouseEnter={() => setIsHovered(true)}
    onMouseLeave={() => setIsHovered(false)}
  >
    <h6 className="flex-center my-2">
      {apps.find(app => app.package_name === package_name)?.label || package_name}
    </h6>
    <iframe
      srcDoc={widget || ""}
      className="grow self-stretch"
      data-widget-code={widget}
    />
    {isHovered && <button
      className="absolute top-0 left-0 icon"
      onClick={() => toggleWidgetVisibility(package_name)}
    >
      {widgetSettings[package_name]?.hide ? <FaEye /> : <FaEyeSlash />}
    </button>}
  </div>
}

export default Widget