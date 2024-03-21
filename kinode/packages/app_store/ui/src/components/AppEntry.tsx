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
    <div {...props} key={appId(app)} className="app-entry row between">
      <AppHeader app={app} size="small" />
      <div className="app-actions row">
        {!app.state?.caps_approved && <ActionButton app={app} style={{ marginRight: "1em" }} />}
        <MoreActions app={app} />
      </div>
    </div>
  );
}
