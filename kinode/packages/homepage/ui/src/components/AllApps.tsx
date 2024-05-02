import useHomepageStore from "../store/homepageStore"
import AppDisplay from "./AppDisplay"

const AllApps: React.FC = () => {
  const { apps } = useHomepageStore()
  return <div className='flex-center flex-wrap gap-8 overflow-y-auto flex-grow self-stretch px-16'>
    {apps.length === 0 ? <div>Loading apps...</div> : apps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AllApps