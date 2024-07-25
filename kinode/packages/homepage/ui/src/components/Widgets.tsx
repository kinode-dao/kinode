import useHomepageStore from "../store/homepageStore"
import Widget from "./Widget"
import usePersistentStore from "../store/persistentStore"

const Widgets = () => {
  const { apps } = useHomepageStore()
  const { widgetSettings } = usePersistentStore();

  return <div>
    {apps
      .filter(app => app.widget)
      .map(({ widget, package_name }, _i, _appsWithWidgets) => !widgetSettings[package_name]?.hide && <Widget
        package_name={package_name}
        widget={widget!}
        forceLarge={_appsWithWidgets.length === 1}
        key={package_name}
      />)}
  </div>
}

export default Widgets