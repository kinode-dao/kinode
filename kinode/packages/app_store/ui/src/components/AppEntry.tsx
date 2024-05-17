import React from "react";

import AppHeader from "./AppHeader";
import ActionButton from "./ActionButton";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import { isMobileCheck } from "../utils/dimensions";
import classNames from "classnames";
import { useNavigate } from "react-router-dom";
import { APP_DETAILS_PATH } from "../constants/path";
import MoreActions from "./MoreActions";

interface AppEntryProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
  size?: "small" | "medium" | "large";
  overrideImageSize?: "small" | "medium" | "large";
  showMoreActions?: boolean;
}

export default function AppEntry({ app, size = "medium", overrideImageSize, showMoreActions, ...props }: AppEntryProps) {
  const isMobile = isMobileCheck()
  const navigate = useNavigate()

  return (
    <div
      {...props}
      key={appId(app)}
      className={classNames("flex justify-between rounded-lg hover:bg-white/10 card cursor-pointer", props.className, {
        'flex-wrap gap-2': isMobile,
        'flex-col relative': size !== 'large'
      })}
      onClick={() => {
        if (!showMoreActions) {
          navigate(`/${APP_DETAILS_PATH}/${appId(app)}`)
        }
      }}
    >
      <AppHeader app={app} size={size} overrideImageSize={overrideImageSize} />
      <ActionButton
        app={app}
        isIcon={!showMoreActions && size !== 'large'}
        className={classNames({
          'absolute': size !== 'large',
          'top-2 right-2': size !== 'large' && showMoreActions,
          'top-0 right-0': size !== 'large' && !showMoreActions,
          'bg-orange text-lg min-w-1/5': size === 'large',
          'ml-auto': size === 'large' && isMobile
        })} />
      {showMoreActions && <div className="absolute bottom-2 right-2">
        <MoreActions app={app} />
      </div>}
    </div>
  );
}
