import React from "react";

import AppHeader from "./AppHeader";
import ActionButton from "./ActionButton";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import { isMobileCheck } from "../utils/dimensions";
import classNames from "classnames";
import { useNavigate } from "react-router-dom";
import { APP_DETAILS_PATH } from "../constants/path";

interface AppEntryProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
  size?: "small" | "medium" | "large";
  overrideImageSize?: "small" | "medium" | "large";
}

export default function AppEntry({ app, size = "medium", overrideImageSize, ...props }: AppEntryProps) {
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
      onClick={() => navigate(`/${APP_DETAILS_PATH}/${appId(app)}`)}
    >
      <AppHeader app={app} size={size} overrideImageSize={overrideImageSize} />
      <ActionButton
        app={app}
        isIcon={size !== 'large'}
        className={classNames({
          'absolute top-0 right-0': size !== 'large',
          'bg-orange text-lg min-w-1/5': size === 'large'
        })} />
    </div>
  );
}
