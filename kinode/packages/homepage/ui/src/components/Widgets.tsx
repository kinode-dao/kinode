import { FaGear } from "react-icons/fa6"
import useHomepageStore from "../store/homepageStore"
import Widget from "./Widget"
import WidgetsSettingsModal from "./WidgetsSettingsModal"

const Widgets = () => {
  const { apps, showWidgetsSettings, setShowWidgetsSettings, widgetSettings } = useHomepageStore()
  return <div className="flex-col-center m-4 gap-4 flex-grow self-stretch">
    <h3 className="flex-center">
      <span>
        Widgets
      </span>
      <button
        className="icon ml-4"
        onClick={() => setShowWidgetsSettings(true)}
      >
        <FaGear />
      </button>
    </h3>
    <div className="flex-center flex-wrap gap-4 flex-grow self-stretch">
      {apps
        .filter(app => app.widget)
        .map(({ widget, package_name }) => !widgetSettings[package_name]?.hide && <Widget
          package_name={package_name}
          widget={widget!}
        />)}
    </div>
    {showWidgetsSettings && <WidgetsSettingsModal />}
  </div>
}

export default Widgets