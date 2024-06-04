import { useEffect, useState } from 'react'
import KinodeText from '../components/KinodeText'
import KinodeBird from '../components/KinodeBird'
import useHomepageStore, { HomepageApp } from '../store/homepageStore'
import { FaChevronDown, FaChevronUp, FaScrewdriverWrench } from 'react-icons/fa6'
import AppsDock from '../components/AppsDock'
import AllApps from '../components/AllApps'
import Widgets from '../components/Widgets'
import { isMobileCheck } from '../utils/dimensions'
import classNames from 'classnames'
import WidgetsSettingsModal from '../components/WidgetsSettingsModal'

import valetIcon from '../../public/valet-icon.png'
import { getFetchUrl } from '../utils/fetch'

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
  const { setApps, isHosted, fetchHostedStatus, showWidgetsSettings, setShowWidgetsSettings } = useHomepageStore()
  const isMobile = isMobileCheck()

  const getAppPathsAndIcons = () => {
    Promise.all([
      fetch(getFetchUrl('/apps'), { credentials: 'include' }).then(res => res.json() as any as HomepageApp[]).catch(() => []),
      fetch(getFetchUrl('/main:app_store:sys/apps'), { credentials: 'include' }).then(res => res.json()).catch(() => []),
      fetch(getFetchUrl('/version'), { credentials: 'include' }).then(res => res.text()).catch(() => '')
    ]).then(([appsData, appStoreData, version]) => {
      console.log({ appsData, appStoreData, version })

      setVersion(version)

      const appz = appsData.map(app => ({
        ...app,
        is_favorite: false, // Assuming initial state for all apps
      }));

      appStoreData.forEach((appStoreApp: AppStoreApp) => {
        const existingAppIndex = appz.findIndex(a => a.package_name === appStoreApp.package);
        if (existingAppIndex === -1) {
          appz.push({
            package_name: appStoreApp.package,
            path: '',
            label: appStoreApp.package,
            state: appStoreApp.state,
            is_favorite: false
          });
        } else {
          appz[existingAppIndex] = {
            ...appz[existingAppIndex],
            state: appStoreApp.state
          };
        }
      });

      setApps(appz);

      // TODO: be less dumb about this edge case!
      for (
        let i = 0;
        i < 5 && appz.find(a => a.package_name === 'app_store' && !a.base64_icon);
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
    fetch(getFetchUrl('/our'), { credentials: 'include' })
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
        {isHosted && <img
          src={valetIcon}
          className='!w-12 !h-12 !p-1 button icon object-cover'
          onClick={() => window.location.href = `https://${our.replace('.os', '')}.hosting.kinode.net/`}
        />}
        <span>{our}</span>
        <span className='bg-white/10 rounded p-1'>v{version}</span>
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
        className={classNames("fixed alt clear flex-center self-center z-20", {
          'bottom-2 right-2': isMobile,
          'bottom-8 right-8': !isMobile,
        })}
        onClick={() => setAllAppsExpanded(!allAppsExpanded)}
      >
        {allAppsExpanded ? <FaChevronDown /> : <FaChevronUp />}
        <span className="ml-2">{allAppsExpanded ? 'Collapse' : 'All apps'}</span>
      </button>
      <AllApps expanded={allAppsExpanded} />
      {showWidgetsSettings && <WidgetsSettingsModal />}
    </div>
  )
}

export default Homepage
