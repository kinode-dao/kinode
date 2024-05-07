import React from "react";

import AppHeader from "./AppHeader";
import ActionButton from "./ActionButton";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import MoreActions from "./MoreActions";
import { isMobileCheck } from "../utils/dimensions";
import classNames from "classnames";

interface AppEntryProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
}

export default function AppEntry({ app, ...props }: AppEntryProps) {
  const isMobile = isMobileCheck()
  return (
    <div {...props} key={appId(app)} className={classNames("flex justify-between w-full rounded hover:bg-white/10 card", {
      'flex-wrap gap-2': isMobile
    })}>
      <AppHeader app={app} size="small" />
      <div className="flex mr-1 items-start">
        <ActionButton app={app} className="mr-2" />
        <MoreActions app={app} />
      </div>
    </div>
  );
}
