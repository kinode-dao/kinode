import React, { useState, useEffect, useCallback, useMemo } from "react";
import { FaChevronLeft, FaChevronRight } from "react-icons/fa";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import AppEntry from "../components/AppEntry";
import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { appId } from "../utils/app";
import classNames from 'classnames';

interface StorePageProps extends PageProps { }

export default function StorePage(props: StorePageProps) {
  // eslint-disable-line
  const { listedApps, getListedApps } = useAppsStore();

  const [resultsSort, setResultsSort] = useState<string>("Recently published");

  const [searchQuery, setSearchQuery] = useState<string>("");
  const [displayedApps, setDisplayedApps] = useState<AppInfo[]>(listedApps);
  const [page, setPage] = useState(1);

  const pages = useMemo(
    () =>
      Array.from(
        { length: Math.ceil(displayedApps.length / 10) },
        (_, index) => index + 1
      ),
    [displayedApps]
  );

  useEffect(() => {
    const start = (page - 1) * 10;
    const end = start + 10;
    setDisplayedApps(listedApps.slice(start, end));
  }, [listedApps]);

  // GET on load
  useEffect(() => {
    getListedApps()
      .then((apps) => {
        setDisplayedApps(Object.values(apps));
      })
      .catch((error) => console.error(error));
  }, []); // eslint-disable-line

  // const pages = useMemo(
  //   () => {
  //     const displayedApps = query ? searchResults : latestApps;

  //     return Array.from(
  //       { length: Math.ceil((displayedApps.length - 2) / 10) },
  //       (_, index) => index + 1
  //     )
  //   },
  //   [query, searchResults, latestApps]
  // );

  // const featuredApps = useMemo(() => latestApps.slice(0, 2), [latestApps]);
  // const displayedApps = useMemo(
  //   () => {
  //     const displayedApps = query ? searchResults : latestApps.slice(2);
  //     return displayedApps.slice((page - 1) * 10, page * 10)
  //   },
  //   [latestApps, searchResults, page, query]
  // );

  const sortApps = useCallback(async (sort: string) => {
    switch (sort) {
      case "Recently published":
        break;
      case "Most popular":
        break;
      case "Best rating":
        break;
      case "Recently updated":
        break;
    }
  }, []);

  // const viewDetails = useCallback(
  //   (app: AppInfo) => () => {
  //     navigate(`/app-details/${appId(app)}`);
  //   },
  //   [navigate]
  // );

  const searchApps = useCallback(
    (query: string) => {
      setSearchQuery(query);
      const filteredApps = listedApps.filter(
        (app) => {
          return (
            app.package.toLowerCase().includes(query.toLowerCase()) ||
            app.metadata?.description
              ?.toLowerCase()
              .includes(query.toLowerCase()) ||
            app.metadata?.description
              ?.toLowerCase()
              .includes(query.toLowerCase())
          );
        },
        [listedApps]
      );
      setDisplayedApps(filteredApps);
    },
    [listedApps]
  );

  return (
    <div className="max-w-[900px] w-full">
      {/* <div style={{ position: "absolute", top: 4, left: 8 }}>
        ID: <strong>{window.our?.node}</strong>
      </div> */}
      <SearchHeader value={searchQuery} onChange={searchApps} />
      {/* <h3 style={{ marginBottom: "0.5em" }}>Featured</h3>
      <div className="featured row">
        {featuredApps.map((app, i) => (
          <div
            key={app.name + app.metadata_hash}
            className="card col"
            style={{
              marginLeft: i === 1 ? "1em" : 0,
              flex: 1,
              cursor: "pointer",
            }}
            onClick={viewDetails(app)}
          >
            <AppHeader app={app} />
            <div style={{ marginTop: "0.25em" }}>
              {app.metadata?.description || "No description provided."}
            </div>
            <ActionButton style={{ marginTop: "0.5em" }} />
          </div>
        ))}
      </div> */}
      <div className="flex justify-between items-center my-2 mx-0">
        <h4>New</h4>

        <select
          value={resultsSort}
          onChange={(e) => {
            setResultsSort(e.target.value);
            sortApps(e.target.value);
          }}
        >
          <option>Recently published</option>
          <option>Most popular</option>
          <option>Best rating</option>
          <option>Recently updated</option>
        </select>
      </div>
      <div className="flex flex-col flex-1 overflow-y-auto gap-2">
        {displayedApps.map((app) => (
          <AppEntry
            key={appId(app) + (app.state?.our_version || "")}
            app={app}
          />
        ))}
        {pages.length > 1 && (
          <div className="flex self-center">
            {page !== pages[0] && (
              <FaChevronLeft onClick={() => setPage(page - 1)} />
            )}
            {pages.map((p) => (
              <div
                key={`page-${p}`}
                className={classNames('my-1 mx-2', { "font-bold": p === page })}
                onClick={() => setPage(p)}
              >
                {p}
              </div>
            ))}
            {page !== pages[pages.length - 1] && (
              <FaChevronRight onClick={() => setPage(page + 1)} />
            )}
          </div>
        )}
      </div>
    </div>
  );
}
