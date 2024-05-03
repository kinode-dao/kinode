import React from "react";

import AppHeader from "./AppHeader";
import ActionButton from "./ActionButton";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import MoreActions from "./MoreActions";

interface AppEntryProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
}

export default function AppEntry({ app, ...props }: AppEntryProps) {
  return (
    <div {...props} key={appId(app)} className="flex justify-between w-full rounded hover:bg-white/10 card">
      <AppHeader app={app} size="small" />
      <div className="flex mr-1 items-start">
        <ActionButton app={app} className="mr-2" />
        <MoreActions app={app} />
      </div>
    </div>
  );
}
