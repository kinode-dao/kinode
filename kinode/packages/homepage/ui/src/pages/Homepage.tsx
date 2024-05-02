import { useEffect, useState } from 'react'
import KinodeText from '../components/KinodeText'
import KinodeBird from '../components/KinodeBird'
import useHomepageStore from '../store/homepageStore'
import { FaGear, FaV } from 'react-icons/fa6'
import AppsDock from '../components/AppsDock'
import AllApps from '../components/AllApps'
import Widgets from '../components/Widgets'

interface AppStoreApp {
  package: string,
  publisher: string,
  state: {
    our_version: string
  }
}
function Homepage() {
  const [our, setOur] = useState('')
  const { apps, setApps, isHosted, fetchHostedStatus } = useHomepageStore()

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
    <div className="flex-col-center relative h-screen w-screen overflow-hidden special-bg-homepage">
      <h5 className='absolute top-8 left-8 right-8 flex gap-4 c'>
        {isHosted && <a
          href={`https://${our.replace('.os', '')}.hosting.kinode.net/`}
          className='button icon'
        >
          <FaV />
        </a>}
        {our}
        <a
          href='/settings:settings:sys'
          className='button icon ml-auto'
        >
          <FaGear />
        </a>
      </h5>
      <div className="flex-col-center gap-6 mt-8 mx-0 mb-16">
        <h3 className='text-center'>Welcome to</h3>
        <KinodeText />
        <KinodeBird />
      </div>
      <AppsDock />
      <Widgets />
      <AllApps />
    </div>
  )
}

export default Homepage
