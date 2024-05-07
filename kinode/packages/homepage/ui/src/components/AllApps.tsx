import classNames from "classnames"
import useHomepageStore from "../store/homepageStore"
import { isMobileCheck } from "../utilities/dimensions"
import AppDisplay from "./AppDisplay"

const AllApps: React.FC<{ expanded: boolean }> = ({ expanded }) => {
  const { apps } = useHomepageStore()
  const isMobile = isMobileCheck()

  return <div className={classNames('flex-center flex-wrap gap-4 overflow-y-auto self-stretch relative', {
    'max-h-0': !expanded,
    'p-8 max-h-[1000px]': expanded,
    'placeholder': isMobile
  })}>
    {apps.length === 0
      ? <div>Loading apps...</div>
      : apps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AllApps