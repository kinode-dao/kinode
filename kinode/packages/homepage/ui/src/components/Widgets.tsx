import useHomepageStore from "../store/homepageStore"

const Widgets = () => {
  const { apps } = useHomepageStore()
  return <div className="flex flex-wrap my-4 gap-4 flex-grow">
    {apps.filter(app => app.widget).map(({ widget }) => <iframe
      srcDoc={widget || ""}
      className="min-w-1/4 shadow-lg"
      data-widget-code={widget}
    />)}
  </div>
}

export default Widgets