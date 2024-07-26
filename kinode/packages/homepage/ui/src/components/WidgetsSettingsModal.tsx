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
    <div className="widget-settings">
      {apps.filter((app) => app.widget).map((app) => {
        return (
          <div>
            <h4>{app.label}</h4>
            <div>
              <div>
                <span>Show widget</span>
                <div>
                  <input
                    type="checkbox"
                    checked={!widgetSettings[app.id]?.hide}
                    onChange={() => toggleWidgetVisibility(app.id)}
                    autoFocus
                  />
                  {!widgetSettings[app.id]?.hide && (
                    <span
                      onClick={() => toggleWidgetVisibility(app.id)}
                      className="checkmark"
                    >
                      &#10003;
                    </span>
                  )}
                </div>
              </div>
              <div>
                <span>Widget size</span>
                <div>
                  <button
                    className={classNames({
                      'clear': widgetSettings[app.package_name]?.size === 'large'
                    })}
                    onClick={() => setWidgetSize(app.package_name, 'small')}
                  >
                    Small
                  </button>
                  <button
                    className={classNames({
                      'clear': widgetSettings[app.package_name]?.size !== 'large'
                    })}
                    onClick={() => setWidgetSize(app.package_name, 'large')}
                  >
                    Large
                  </button>
                </div>
              </div>
            </div>
          </div>
        );
      })}
      <button onClick={() => window.location.href = '/settings:settings:sys'}>
        System settings
      </button>
    </div>
  </Modal>
}

export default WidgetsSettingsModal