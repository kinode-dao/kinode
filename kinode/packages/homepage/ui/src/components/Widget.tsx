import { useEffect, useState } from "react"
import useHomepageStore from "../store/homepageStore"

interface WidgetProps {
  package_name: string,
  widget: string
  forceLarge?: boolean
}

const Widget: React.FC<WidgetProps> = ({ package_name, widget }) => {
  const { apps } = useHomepageStore()
  const [_tallScreen, setTallScreen] = useState(window.innerHeight > window.innerWidth)

  useEffect(() => {
    setTallScreen(window.innerHeight > window.innerWidth)
  }, [window.innerHeight, window.innerWidth])

  return <div>
    <h6>
      {apps.find(app => app.package_name === package_name)?.label || package_name}
    </h6>
    <iframe
      srcDoc={widget || ""}
      data-widget-code={widget}
    />
  </div>
}

export default Widget