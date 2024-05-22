import classNames from "classnames"
import useHomepageStore from "../store/homepageStore"
import { isMobileCheck } from "../utils/dimensions"
import AppDisplay from "./AppDisplay"

const AllApps: React.FC<{ expanded: boolean }> = ({ expanded }) => {
  const { apps } = useHomepageStore()
  const isMobile = isMobileCheck()

  return <div className={classNames('flex-center flex-wrap overflow-y-auto fixed h-screen w-screen backdrop-blur-md transition transition-all ease-in-out duration-500', {
    'top-[100vh]': !expanded,
    'top-0': expanded,
    'gap-4 p-8': isMobile,
    'gap-8 p-16': !isMobile,
  })}>
    {apps.length === 0
      ? <div>Loading apps...</div>
      : apps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AllApps