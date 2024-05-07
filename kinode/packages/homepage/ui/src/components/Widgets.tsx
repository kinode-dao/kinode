import { FaGear } from "react-icons/fa6"
import useHomepageStore from "../store/homepageStore"
import Widget from "./Widget"
import WidgetsSettingsModal from "./WidgetsSettingsModal"
import usePersistentStore from "../store/persistentStore"
import { isMobileCheck } from "../utilities/dimensions"
import classNames from "classnames"

const Widgets = () => {
  const { apps, showWidgetsSettings, setShowWidgetsSettings } = useHomepageStore()
  const { widgetSettings } = usePersistentStore();
  const isMobile = isMobileCheck()

  return <div className={classNames("flex-col-center flex-grow self-stretch relative", {
    'm-4': !isMobile,
    'm-2': isMobile
  })}>
    <button
      className="icon ml-4 absolute top-0 right-4"
      onClick={() => setShowWidgetsSettings(true)}
    >
      <FaGear />
    </button>
    <div className={classNames("flex-center flex-wrap flex-grow self-stretch", {
      'gap-2': isMobile,
      'gap-4': !isMobile
    })}>
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