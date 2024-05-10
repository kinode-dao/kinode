import React, { useState, useEffect, useCallback, useMemo } from "react";
import { FaChevronLeft, FaChevronRight } from "react-icons/fa";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import AppEntry from "../components/AppEntry";
import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { appId } from "../utils/app";
import classNames from 'classnames';
import { FaArrowRotateRight } from "react-icons/fa6";
import { isMobileCheck } from "../utils/dimensions";
import HomeButton from "../components/HomeButton";

interface StorePageProps extends PageProps { }

export default function StorePage() {
  // eslint-disable-line
  const { listedApps, getListedApps, rebuildIndex } = useAppsStore();

  const [resultsSort, setResultsSort] = useState<string>("Recently published");

  const [searchQuery, setSearchQuery] = useState<string>("");
  const [displayedApps, setDisplayedApps] = useState<AppInfo[]>(listedApps);
  const [page, setPage] = useState(1);
  const [tags, setTags] = useState<string[]>([])

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
        let _tags: string[] = [];
        for (const app of Object.values(apps)) {
          _tags = _tags.concat((app.metadata as any || {}).tags || [])
        }
        if (_tags.length === 0) {
          _tags = ['App', 'Tags', 'Coming', 'Soon', 'tm'];
        }
        setTags(Array.from(new Set(_tags)))
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

  const tryRebuildIndex = useCallback(async () => {
    try {
      await rebuildIndex();
      alert("Index rebuilt successfully.");
      await getListedApps();
    } catch (error) {
      console.error(error);
    }
  }, [rebuildIndex]);

  const isMobile = isMobileCheck()

  return (
    <div className={classNames("flex flex-col max-w-[900px] w-full", {
      'gap-4': isMobile,
      'gap-6': !isMobile
    })}>
      <HomeButton />
      <SearchHeader value={searchQuery} onChange={searchApps} />
      <div className={classNames("flex items-center self-stretch justify-between", {
        'gap-4': isMobile,
        'gap-8': !isMobile
      })}>
        <button
          className="flex flex-col c icon icon-orange"
          onClick={tryRebuildIndex}
          title="Rebuild index"
        >
          <FaArrowRotateRight />
        </button>

        {tags.slice(0, isMobile ? 3 : 6).map(tag => (
          <button
            key={tag}
            className="clear flex c rounded-full !bg-white/10 !hover:bg-white/25"
            onClick={() => {
              console.log('clicked tag', tag)
            }}
          >
            {tag}
          </button>
        ))}

        <select
          value={resultsSort}
          onChange={(e) => {
            setResultsSort(e.target.value);
            sortApps(e.target.value);
          }}
          className={classNames({
            'basis-1/5': !isMobile
          })}
        >
          <option>Recently published</option>
          <option>Most popular</option>
          <option>Best rating</option>
          <option>Recently updated</option>
        </select>
      </div>
      {!searchQuery ? <>
        <h2>Top apps this week...</h2>
        <div className={classNames("flex gap-2", {
          'flex-wrap': isMobile
        })}>
          {displayedApps.slice(0, 4).map((app) => (
            <AppEntry
              key={appId(app) + (app.state?.our_version || "")}
              size='medium'
              app={app}
              className={classNames("grow", {
                'w-1/4': !isMobile,
                'w-1/2': isMobile
              })}
            />
          ))}
        </div>
        <h2>Must-have apps!</h2>
        <div className={classNames("flex gap-2", {
          'flex-wrap': isMobile
        })}>
          {displayedApps.slice(0, 6).map((app) => (
            <AppEntry
              key={appId(app) + (app.state?.our_version || "")}
              size='small'
              app={app}
              overrideImageSize="large"
              className={classNames("grow", {
                'w-1/6': !isMobile,
                'w-1/2': isMobile
              })}
            />
          ))}
        </div>
      </> : <div className={classNames("flex-col-center", {
        'gap-2': isMobile,
        'gap-4': !isMobile,
      })}>
        {displayedApps.map(app => <AppEntry
          size='large'
          app={app}
          className="self-stretch items-center"
          overrideImageSize="medium"
        />)}
      </div>}
      <div className="flex flex-col gap-2 overflow-y-auto">
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
