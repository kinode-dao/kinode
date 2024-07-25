import { useEffect, useState } from 'react'
import KinodeText from '../components/KinodeText'
import KinodeBird from '../components/KinodeBird'
import useHomepageStore, { HomepageApp } from '../store/homepageStore'
import { FaChevronDown, FaChevronUp, FaScrewdriverWrench } from 'react-icons/fa6'
import AppsDock from '../components/AppsDock'
import AllApps from '../components/AllApps'
import Widgets from '../components/Widgets'
import WidgetsSettingsModal from '../components/WidgetsSettingsModal'

interface AppStoreApp {
  package: string,
  publisher: string,
  state: {
    our_version: string
  }
}
function Homepage() {
  const [our, setOur] = useState('')
  const [version, setVersion] = useState('')
  const [allAppsExpanded, setAllAppsExpanded] = useState(false)
  const { setApps, showWidgetsSettings, setShowWidgetsSettings } = useHomepageStore()

  const getAppPathsAndIcons = () => {
    Promise.all([
      fetch('/apps', { credentials: 'include' }).then(res => res.json() as any as HomepageApp[]).catch(() => []),
      fetch('/main:app_store:sys/apps', { credentials: 'include' }).then(res => res.json()).catch(() => []),
      fetch('/version', { credentials: 'include' }).then(res => res.text()).catch(() => '')
    ]).then(([appsData, appStoreData, version]) => {

      setVersion(version)

      const apps = appsData.map(app => ({
        ...app,
        is_favorite: false, // Assuming initial state for all apps
      }));

      appStoreData.forEach((appStoreApp: AppStoreApp) => {
        const existingAppIndex = apps.findIndex(a => a.package_name === appStoreApp.package);
        if (existingAppIndex === -1) {
          apps.push({
            package_name: appStoreApp.package,
            path: '',
            label: appStoreApp.package,
            state: appStoreApp.state,
            is_favorite: false
          });
        } else {
          apps[existingAppIndex] = {
            ...apps[existingAppIndex],
            state: appStoreApp.state
          };
        }
      });

      setApps(apps);

      // TODO: be less dumb about this edge case!
      for (
        let i = 0;
        i < 5 && apps.find(a => a.package_name === 'app_store' && !a.base64_icon);
        i++
      ) {
        getAppPathsAndIcons();
      }
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
