import { useEffect, useState } from 'react'
import KinodeText from './components/KinodeText'
import KinodeBird from './components/KinodeBird'
import useHomepageStore from './store/homepageStore'
import { FaV } from 'react-icons/fa6'

interface HomepageApp {
  package_name: string,
  path: string
  label: string,
  base64_icon: string,
}
function App() {
  const [our, setOur] = useState('')
  const [apps, setApps] = useState<HomepageApp[]>([])
  const { isHosted, fetchHostedStatus } = useHomepageStore()

  useEffect(() => {
    fetch('/apps')
      .then(res => res.json())
      .then(data => setApps(data))
    fetch('/our')
      .then(res => res.text())
      .then(data => setOur(data))
      .then(() => fetchHostedStatus(our))
  }, [our])

  return (
    <div className="flex-col-center relative h-screen w-screen overflow-hidden special-bg-homepage">
      <h5 className='absolute top-8 left-8'>
        {isHosted && <a
          href={`https://${our.replace('.os', '')}.hosting.kinode.net/`}
          className='button icon'
        >
          <FaV />
        </a>}
        {our}
      </h5>
      <div className="flex-col-center gap-6 mt-8 mx-0 mb-16">
        <h3 className='text-center'>Welcome to</h3>
        <KinodeText />
        <KinodeBird />
      </div>
      <div className='flex-center flex-wrap gap-8'>
        {apps.length === 0 ? <div>Loading apps...</div> : apps.map(app => <a
          className="flex-col-center mb-8 cursor-pointer gap-2 hover:opacity-90"
          id={app.package_name}
          href={app.path}
        >
          <img
            src={app.base64_icon}
            className='h-32 w-32'
          />
          <h6>{app.label}</h6>
        </a>)}
      </div>
    </div>
  )
}

export default App
