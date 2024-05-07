import { FaCheck, FaX } from "react-icons/fa6"
import useHomepageStore from "../store/homepageStore"
import { Modal } from "./Modal"
import classNames from "classnames"
import usePersistentStore from "../store/persistentStore"

const WidgetsSettingsModal = () => {
  const { apps, setShowWidgetsSettings } = useHomepageStore()
  const { widgetSettings, toggleWidgetVisibility, setWidgetSize } = usePersistentStore()

  return <Modal
    title='Widget Settings'
    onClose={() => setShowWidgetsSettings(false)}
  >
    <div className="flex-col-center gap-4 mt-4">
      {apps.filter(app => app.widget).map(({ label, package_name }) => <div className="flex items-start bg-white/10 rounded p-2 self-stretch">
        <h4 className="mr-4">{label}</h4>
        <div className="flex-col-center gap-4 grow">
          <div className="flex-center gap-2">
            <span>Show widget</span>
            <button
              className="icon"
              onClick={() => toggleWidgetVisibility(package_name)}
            >
              {!widgetSettings[package_name]?.hide ? <FaCheck /> : <FaX />}
            </button>
          </div>
          <div className="flex-center gap-2">
            <span>Widget size</span>
            <button
              className={classNames({
                'clear': widgetSettings[package_name]?.size === 'large'
              })}
              onClick={() => setWidgetSize(package_name, 'small')}
            >
              Small
            </button>
            <button
              className={classNames({
                'clear': widgetSettings[package_name]?.size !== 'large'
              })}
              onClick={() => setWidgetSize(package_name, 'large')}
            >
              Large
            </button>
          </div>
        </div>
      </div>)}
    </div>
  </Modal>
}

export default WidgetsSettingsModal