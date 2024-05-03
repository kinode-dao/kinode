import classNames from "classnames"
import useHomepageStore from "../store/homepageStore"
import { FaEye, FaEyeSlash } from "react-icons/fa6"
import { useState } from "react"

interface WidgetProps {
  package_name: string,
  widget: string
}

const Widget: React.FC<WidgetProps> = ({ package_name, widget }) => {
  const { widgetSettings, toggleWidgetVisibility } = useHomepageStore()
  const [isHovered, setIsHovered] = useState(false)
  return <div
    className={classNames("self-stretch flex flex-wrap shadow-lg rounded-lg relative", {
      "min-w-full": widgetSettings[package_name]?.size === "large",
      "min-w-1/2": !widgetSettings[package_name]?.size || widgetSettings[package_name]?.size === "small"
    })}
    onMouseEnter={() => setIsHovered(true)}
    onMouseLeave={() => setIsHovered(false)}
  >
    <iframe
      srcDoc={widget || ""}
      className="grow self-stretch"
      data-widget-code={widget}
    />
    {isHovered && <button
      className="absolute -top-2 -right-2 icon"
      onClick={() => toggleWidgetVisibility(package_name)}
    >
      {widgetSettings[package_name]?.hide ? <FaEye /> : <FaEyeSlash />}
    </button>}
  </div>
}

export default Widget