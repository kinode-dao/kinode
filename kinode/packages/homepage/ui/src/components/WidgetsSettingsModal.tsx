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
        <h4 className="mr-4 grow">{label}</h4>
        <div className="flex flex-col gap-4 grow">
          <div className="flex-center gap-2">
            <span>Show widget</span>
            <div className="flex relative grow">
              <input
                type="checkbox"
                checked={!widgetSettings[package_name]?.hide}
                onChange={() => toggleWidgetVisibility(package_name)}
                autoFocus
              />
              {!widgetSettings[package_name]?.hide && (
                <span
                  onClick={() => toggleWidgetVisibility(package_name)}
                  className="checkmark"
                >
                  &#10003;
                </span>
              )}
            </div>
          </div>
          <div className="flex-center gap-2">
            <span>Widget size</span>
            <div className="flex-center grow">
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
        </div>
      </div>)}
      <button
        className="clear"
        onClick={() => window.location.href = '/settings:settings:sys'}
      >
        Looking for system settings?
      </button>
    </div>
  </Modal>
}

export default WidgetsSettingsModal