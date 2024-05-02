import { HomepageApp } from "../store/homepageStore"
import AppDisplay from "./AppDisplay"

interface AppsDockProps {
  apps: HomepageApp[]
}

const AppsDock: React.FC<AppsDockProps> = ({ apps }) => {
  return <div className='flex-center flex-wrap obox border border-orange gap-8'>
    {apps.length === 0 ? <div>Favorite an app to pin it to your dock.</div> : apps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AppsDock