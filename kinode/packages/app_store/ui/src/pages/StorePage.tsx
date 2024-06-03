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
        {
          length: Math.ceil(listedApps.length / 10)
        },
        (_, index) => index + 1
      ),
    [listedApps]
  );

  const featuredPackageNames = ['dartfrog', 'kcal', 'memedeck', 'filter'];

  useEffect(() => {
    const start = (page - 1) * 10;
    const end = start + 10;
    setDisplayedApps(listedApps.slice(start, end));
  }, [listedApps, page]);

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
    <div className={classNames("flex flex-col w-full max-h-screen p-2", {
      'gap-4 max-w-screen': isMobile,
      'gap-6 max-w-[900px]': !isMobile
    })}>
      {!isMobile && <HomeButton />}
      <SearchHeader value={searchQuery} onChange={searchApps} />
      <div className={classNames("flex items-center self-stretch justify-between", {
        'gap-4 flex-wrap': isMobile,
        'gap-8 grow': !isMobile
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
          className={classNames('hidden', {
            'basis-1/5': !isMobile
          })}
        >
          <option>Recently published</option>
          <option>Most popular</option>
          <option>Best rating</option>
          <option>Recently updated</option>
        </select>
      </div>
      {!searchQuery && <div className={classNames("flex flex-col", {
        'gap-4': !isMobile,
        'grow overflow-y-auto gap-2 items-center px-2': isMobile
      })}>
        <h2>Featured Apps</h2>
        <div className={classNames("flex gap-2", {
          'flex-col': isMobile
        })}>
          {listedApps.filter(app => {
            return featuredPackageNames.indexOf(app.package) !== -1
          }).map((app) => (
            <AppEntry
              key={appId(app) + (app.state?.our_version || "")}
              size={'medium'}
              app={app}
              className={classNames("grow", {
                'w-1/4': !isMobile,
                'w-full': isMobile
              })}
            />
          ))}
        </div>
      </div>}
      <h2>{searchQuery ? 'Search Results' : 'All Apps'}</h2>
      <div className={classNames("flex flex-col grow overflow-y-auto", {
        'gap-2': isMobile,
        'gap-4': !isMobile,
      })}>
        {displayedApps
          .filter(app => searchQuery ? true : featuredPackageNames.indexOf(app.package) === -1)
          .map(app => <AppEntry
            key={appId(app) + (app.state?.our_version || "")}
            size='large'
            app={app}
            className="self-stretch"
            overrideImageSize="medium"
          />)}
      </div>
      {pages.length > 1 && <div className="flex flex-wrap self-center gap-2">
        <button
          className="icon"
          onClick={() => page !== pages[0] && setPage(page - 1)}
        >
          <FaChevronLeft />
        </button>
        {pages.map((p) => (
          <button
            key={`page-${p}`}
            className={classNames('icon', { "!bg-white/10": p === page })}
            onClick={() => setPage(p)}
          >
            {p}
          </button>
        ))}
        <button
          className="icon"
          onClick={() => page !== pages[pages.length - 1] && setPage(page + 1)}
        >
          <FaChevronRight />
        </button>
      </div>}
    </div>
  );
}
