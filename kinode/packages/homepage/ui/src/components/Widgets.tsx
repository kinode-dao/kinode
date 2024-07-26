import useHomepageStore from "../store/homepageStore"
import Widget from "./Widget"
import usePersistentStore from "../store/persistentStore"

const Widgets = () => {
  const { apps } = useHomepageStore()
  const { widgetSettings } = usePersistentStore();

  return <div id="widgets">
    {apps.filter((app) => app.widget && !widgetSettings[app.id]?.hide).map((app) => <Widget
      label={app.label}
      widget={app.widget!}
      key={app.id}
    />)}
  </div>
}

export default Widgets