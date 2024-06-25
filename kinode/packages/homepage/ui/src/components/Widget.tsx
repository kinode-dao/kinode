import classNames from "classnames"
import { useEffect, useState } from "react"
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
  const { widgetSettings } = usePersistentStore()
  const isMobile = isMobileCheck()
  const isLarge = forceLarge || widgetSettings[package_name]?.size === "large"
  const isSmall = !widgetSettings[package_name]?.size || widgetSettings[package_name]?.size === "small"
  const [tallScreen, setTallScreen] = useState(window.innerHeight > window.innerWidth)

  useEffect(() => {
    setTallScreen(window.innerHeight > window.innerWidth)
  }, [window.innerHeight, window.innerWidth])

  return <div
    className={classNames("self-stretch flex-col-center shadow-lg rounded-lg relative", {
      "max-w-1/2 min-w-1/2": isLarge && !isMobile,
      "min-w-1/4": isSmall && !isMobile,
      "max-w-1/4": isSmall && !tallScreen,
      'w-full': isMobile
    })}
  >
    <h6 className="flex-center my-2">
      {apps.find(app => app.package_name === package_name)?.label || package_name}
    </h6>
    <iframe
      srcDoc={widget || ""}
      className="grow self-stretch"
      data-widget-code={widget}
    />
  </div>
}

export default Widget