import { useEffect, useState } from 'react'
import KinodeText from '../components/KinodeText'
import KinodeBird from '../components/KinodeBird'
import useHomepageStore, { HomepageApp } from '../store/homepageStore'
import { FaChevronDown, FaChevronUp, FaScrewdriverWrench } from 'react-icons/fa6'
import AppsDock from '../components/AppsDock'
import AllApps from '../components/AllApps'
import Widgets from '../components/Widgets'
import WidgetsSettingsModal from '../components/WidgetsSettingsModal'

function Homepage() {
  const [our, setOur] = useState('')
  const [version, setVersion] = useState('')
  const [allAppsExpanded, setAllAppsExpanded] = useState(false)
  const { setApps, showWidgetsSettings, setShowWidgetsSettings } = useHomepageStore()

  const getAppPathsAndIcons = () => {
    Promise.all([
      fetch('/apps', { credentials: 'include' }).then(res => res.json() as any as HomepageApp[]).catch(() => []),
      fetch('/version', { credentials: 'include' }).then(res => res.text()).catch(() => '')
    ]).then(([appsData, version]) => {

      setVersion(version)

      const apps = appsData.map(app => ({
        ...app,
      }));

      setApps(apps);
    });
  }

  useEffect(() => {
    getAppPathsAndIcons();
  }, [our]);

  useEffect(() => {
    fetch('/our', { credentials: 'include' })
      .then(res => res.text())
      .then(data => {
        if (data.match(/^[a-zA-Z0-9\-\.]+\.[a-zA-Z]+$/)) {
          setOur(data)
        }
      })
  }, [our])

  return (
    <div>
      <h5>
        <span>Hello, {our}</span>
        <span>v{version}</span>
        <button onClick={() => setShowWidgetsSettings(true)}>
          <FaScrewdriverWrench />
        </button>
      </h5>
      <div>
        <KinodeBird />
        <KinodeText />
      </div>
      <AppsDock />
      <Widgets />
      <button onClick={() => setAllAppsExpanded(!allAppsExpanded)}>
        {allAppsExpanded ? <FaChevronDown /> : <FaChevronUp />}
        <span>{allAppsExpanded ? 'Collapse' : 'All apps'}</span>
      </button>
      <AllApps expanded={allAppsExpanded} />
      {showWidgetsSettings && <WidgetsSettingsModal />}
    </div>
  )
}

export default Homepage
