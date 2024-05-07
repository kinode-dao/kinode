import classNames from "classnames";
import kinodeLogo from "../assets/kinode.svg";
import { isMobileCheck } from "../utils/dimensions";

export const KinodeTitle: React.FC<{ prefix: string, showLogo?: boolean }> = ({ prefix, showLogo }) => {
  const isMobile = isMobileCheck()

  return (
    <div className="mb-4 flex flex-col c">
      <h1>{prefix}</h1>
      {showLogo && <>
        <h1
          className={classNames("display", {
            'text-5xl mt-10 mb-8 ml-4': !isMobile,
            'text-3xl mt-5 mb-4 ml-2': isMobile
          })}>Kinode<span className="text-xs">&reg;</span></h1>
        <img src={kinodeLogo} className={classNames({
          'w-32 h-32': !isMobile,
          'w-16 h-16': isMobile,
        })} />
      </>}
    </div>
  );
};
