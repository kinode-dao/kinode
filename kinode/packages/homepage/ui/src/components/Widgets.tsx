import { FaGear } from "react-icons/fa6"
import useHomepageStore from "../store/homepageStore"
import Widget from "./Widget"
import WidgetsSettingsModal from "./WidgetsSettingsModal"
import usePersistentStore from "../store/persistentStore"

const Widgets = () => {
  const { apps, showWidgetsSettings, setShowWidgetsSettings } = useHomepageStore()
  const { widgetSettings } = usePersistentStore();
  return <div className="flex-col-center m-4 gap-4 flex-grow self-stretch relative">
    <button
      className="icon ml-4 absolute top-0 right-4"
      onClick={() => setShowWidgetsSettings(true)}
    >
      <FaGear />
    </button>
    <div className="flex-center flex-wrap gap-4 flex-grow self-stretch">
      {apps
        .filter(app => app.widget)
        .map(({ widget, package_name }, _i, _appsWithWidgets) => !widgetSettings[package_name]?.hide && <Widget
          package_name={package_name}
          widget={widget!}
          forceLarge={_appsWithWidgets.length === 1}
        />)}
    </div>
    {showWidgetsSettings && <WidgetsSettingsModal />}
  </div>
}

export default Widgets