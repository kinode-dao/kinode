import { useEffect, useState } from "react"
import usePersistentStore from "../store/persistentStore"

interface WidgetProps {
  id: string
  label: string
  widget: string
}

const Widget: React.FC<WidgetProps> = ({ id, label, widget }) => {
  const [_tallScreen, setTallScreen] = useState(window.innerHeight > window.innerWidth)
  const { toggleWidgetVisibility } = usePersistentStore();

  useEffect(() => {
    setTallScreen(window.innerHeight > window.innerWidth)
  }, [window.innerHeight, window.innerWidth])

  useEffect(() => {
    const hideWidget = () => {
      toggleWidgetVisibility(id)
    }
    document.getElementById(`hide-widget-${id}`)?.addEventListener("click", hideWidget)
  }, [])

  return <div className="widget">
    <div className="bottom-bar"><p>{label}</p><p id={`hide-widget-${id}`}>[hide]</p></div>
    <iframe srcDoc={widget} />
  </div>
}

export default Widget