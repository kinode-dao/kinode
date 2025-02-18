import useHomepageStore from "../store/homepageStore"
import { Modal } from "./Modal"
import usePersistentStore from "../store/persistentStore"

const WidgetsSettingsModal = () => {
  const { apps, setShowWidgetsSettings } = useHomepageStore()
  const { widgetSettings, toggleWidgetVisibility } = usePersistentStore()

  return <Modal
    title='Widget Settings'
    onClose={() => setShowWidgetsSettings(false)}
  >
    <div className="widget-settings">
      {apps.filter((app) => app.widget).map((app) => {
        return (
          <div className="widget-settings-item">
            <h4>{app.label}</h4>
            <div>
              <div>
                <span><input
                  type="checkbox"
                  checked={!widgetSettings[app.id]?.hide}
                  onChange={() => toggleWidgetVisibility(app.id)}
                  autoFocus
                /></span>
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