import { useEffect, useState } from 'react'
import KinodeText from '../components/KinodeText'
import KinodeBird from '../components/KinodeBird'
import useHomepageStore from '../store/homepageStore'
import { FaChevronDown, FaChevronUp, FaScrewdriverWrench, FaV } from 'react-icons/fa6'
import AppsDock from '../components/AppsDock'
import AllApps from '../components/AllApps'
import Widgets from '../components/Widgets'
import { isMobileCheck } from '../utilities/dimensions'
import classNames from 'classnames'
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
  const [allAppsExpanded, setAllAppsExpanded] = useState(false)
  const { apps, setApps, isHosted, fetchHostedStatus, showWidgetsSettings, setShowWidgetsSettings } = useHomepageStore()
  const isMobile = isMobileCheck()

  useEffect(() => {
    fetch('/apps')
      .then(res => res.json())
      .then(data => setApps(data))
      .then(() => {
        fetch('/main:app_store:sys/apps')
          .then(res => res.json())
          .then(data => {
            const appz = [...apps]
            data.forEach((app: AppStoreApp) => {
              if (!appz.find(a => a.package_name === app.package)) {
                appz.push({
                  package_name: app.package,
                  path: '',
                  label: app.package,
                  state: app.state,
                  is_favorite: false
                })
              } else {
                const i = appz.findIndex(a => a.package_name === app.package)
                if (i !== -1) {
                  appz[i] = { ...appz[i], state: app.state }
                }
              }
            })
            setApps(appz)
          })
      })
    fetch('/our')
      .then(res => res.text())
      .then(data => {
        if (data.match(/^[a-zA-Z0-9\-\.]+\.[a-zA-Z]+$/)) {
          setOur(data)
          fetchHostedStatus(data)
        }
      })
  }, [our])

  return (
    <div className={classNames("flex-col-center relative w-screen overflow-hidden special-bg-homepage min-h-screen", {
    })}>
      <h5 className={classNames('absolute flex gap-4 c', {
        'top-8 left-8 right-8': !isMobile,
        'top-2 left-2 right-2': isMobile
      })}>
        {isHosted && <a
          href={`https://${our.replace('.os', '')}.hosting.kinode.net/`}
          className='button icon'
        >
          <FaV />
        </a>}
        {our}
        <button
          className="icon ml-auto"
          onClick={() => setShowWidgetsSettings(true)}
        >
          <FaScrewdriverWrench />
        </button>
      </h5>
      {isMobile
        ? <div className='flex-center gap-4 p-8 mt-8 max-w-screen'>
          <KinodeBird />
          <KinodeText />
        </div>
        : <div className={classNames("flex-col-center mx-0 gap-4 mt-8 mb-4")}>
          <h3 className='text-center'>Welcome to</h3>
          <KinodeText />
          <KinodeBird />
        </div>}
      <AppsDock />
      <Widgets />
      <button
        className={classNames("clear flex-center self-center", {
          '-mb-1': !allAppsExpanded,
        })}
        onClick={() => setAllAppsExpanded(!allAppsExpanded)}
      >
        {allAppsExpanded ? <FaChevronDown /> : <FaChevronUp />}
        <span className="ml-2">{allAppsExpanded ? 'Collapse' : 'All installed apps'}</span>
      </button>
      <AllApps expanded={allAppsExpanded} />
      {showWidgetsSettings && <WidgetsSettingsModal />}
    </div>
  )
}

export default Homepage
