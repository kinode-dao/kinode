import useHomepageStore from "../store/homepageStore"
import Widget from "./Widget"
import usePersistentStore from "../store/persistentStore"
import { isMobileCheck } from "../utilities/dimensions"
import classNames from "classnames"

const Widgets = () => {
  const { apps } = useHomepageStore()
  const { widgetSettings } = usePersistentStore();
  const isMobile = isMobileCheck()

  return <div className={classNames("flex-center flex-wrap flex-grow self-stretch", {
    'gap-2 m-2': isMobile,
    'gap-4 m-4': !isMobile
  })}>
    {apps
      .filter(app => app.widget)
      .map(({ widget, package_name }, _i, _appsWithWidgets) => !widgetSettings[package_name]?.hide && <Widget
        package_name={package_name}
        widget={widget!}
        forceLarge={_appsWithWidgets.length === 1}
      />)}
  </div>
}

export default Widgets