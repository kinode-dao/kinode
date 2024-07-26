import { useEffect, useState } from "react"

interface WidgetProps {
  label: string
  widget: string
}

const Widget: React.FC<WidgetProps> = ({ label, widget }) => {
  const [_tallScreen, setTallScreen] = useState(window.innerHeight > window.innerWidth)

  useEffect(() => {
    setTallScreen(window.innerHeight > window.innerWidth)
  }, [window.innerHeight, window.innerWidth])

  return <div id="widget">
    <p>{label}</p>
    <iframe srcDoc={widget} />
  </div>
}

export default Widget