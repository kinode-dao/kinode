import { useEffect, useState } from 'react'
import KinodeBird from '../components/KinodeBird'
import useHomepageStore from '../store/homepageStore'
import { FaChevronDown, FaChevronUp } from 'react-icons/fa6'
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
      fetch('/apps', { credentials: 'include' }).then(res => res.json()).catch(() => []),
      fetch('/version', { credentials: 'include' }).then(res => res.text()).catch(() => '')
    ]).then(([appsData, version]) => {
      setVersion(version)
      setApps(appsData)
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

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && allAppsExpanded) {
        setAllAppsExpanded(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [allAppsExpanded]);

  return (
    <div id="homepage">
      <header>
        <KinodeBird />
        <h2>{new Date().getHours() < 4
          ? 'Good evening'
          : new Date().getHours() < 12
            ? 'Good morning'
            : new Date().getHours() < 18
              ? 'Good afternoon'
              : 'Good evening'}, {our}</h2>
        <a href="https://github.com/kinode-dao/kinode/releases" target="_blank">[kinode v{version}]</a>
        <a href="#" onClick={(e) => { e.preventDefault(); setShowWidgetsSettings(true); }}>
          [⚙]
        </a>
      </header>
      <AppsDock />
      <Widgets />
      <footer>
        <button className="footer-button" onClick={() => setAllAppsExpanded(!allAppsExpanded)}>
          {allAppsExpanded ? <FaChevronDown /> : <FaChevronUp />}
          <span>{allAppsExpanded ? 'Collapse' : 'All apps'}</span>
        </button>
        <div style={{ display: allAppsExpanded ? 'block' : 'none' }}>
          <AllApps expanded={allAppsExpanded} />
        </div>
      </footer>
      {showWidgetsSettings && <WidgetsSettingsModal />}
    </div >
  )
}

export default Homepage
