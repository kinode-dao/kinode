import React, { useState, useEffect, useCallback, useMemo } from "react";
import { FaChevronLeft, FaChevronRight } from "react-icons/fa";
import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import AppEntry from "../components/AppEntry";
import SearchHeader from "../components/SearchHeader";
import { appId } from "../utils/app";
import HomeButton from "../components/HomeButton";
import Modal from "../components/Modal";
import Loader from "../components/Loader";

export default function StorePage() {
  const { listedApps, getListedApps, rebuildIndex } = useAppsStore();
  const [resultsSort, setResultsSort] = useState<string>("Recently published");
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [displayedApps, setDisplayedApps] = useState<AppInfo[]>(listedApps);
  const [page, setPage] = useState(1);
  const [tags, setTags] = useState<string[]>([]);
  const [launchPaths, setLaunchPaths] = useState<{ [package_name: string]: string }>({});
  const [isRebuildingIndex, setIsRebuildingIndex] = useState(false);

  const pages = useMemo(() => Array.from({ length: Math.ceil(listedApps.length / 10) }, (_, index) => index + 1), [listedApps]);
  const featuredPackageNames = ['dartfrog', 'kcal', 'memedeck', 'filter'];

  useEffect(() => {
    const start = (page - 1) * 10;
    const end = start + 10;
    setDisplayedApps(listedApps.slice(start, end));
  }, [listedApps, page]);

  useEffect(() => {
    getListedApps()
      .then((apps) => {
        setDisplayedApps(Object.values(apps));
        let _tags: string[] = [];
        for (const app of Object.values(apps)) {
          _tags = _tags.concat((app.metadata as any || {}).tags || []);
        }
        if (_tags.length === 0) {
          _tags = ['App', 'Tags', 'Coming', 'Soon', 'tm'];
        }
        setTags(Array.from(new Set(_tags)));
      })
      .catch((error) => console.error(error));
  }, []);

  const sortApps = useCallback(async (sort: string) => {
    // Implement sorting logic here
  }, []);

  const searchApps = useCallback((query: string) => {
    setSearchQuery(query);
    const filteredApps = listedApps.filter((app) => {
      return (
        app.package.toLowerCase().includes(query.toLowerCase()) ||
        app.metadata?.description?.toLowerCase().includes(query.toLowerCase())
      );
    });
    setDisplayedApps(filteredApps);
  }, [listedApps]);

  const tryRebuildIndex = useCallback(async () => {
    if (!window.confirm('Are you sure you want to rebuild the app index? This may take a few seconds.')) {
      return;
    }
    setIsRebuildingIndex(true);
    try {
      await rebuildIndex();
      await getListedApps();
    } catch (error) {
      console.error(error);
    } finally {
      setIsRebuildingIndex(false);
    }
  }, [rebuildIndex, getListedApps]);

  useEffect(() => {
    fetch('/main:app_store:sys/apps')
      .then(data => data.json())
      .then((data: Array<{ package_name: string, path: string }>) => {
        if (Array.isArray(data)) {
          const newLaunchPaths = { ...launchPaths };
          listedApps.forEach(app => {
            const homepageAppData = data.find(otherApp => app.package === otherApp.package_name);
            if (homepageAppData) {
              newLaunchPaths[app.package] = homepageAppData.path;
            }
          });
          setLaunchPaths(newLaunchPaths);
        }
      });
  }, [listedApps]);

  return (
    <div className="store-page">
      <HomeButton />
      <SearchHeader value={searchQuery} onChange={searchApps} />
      <div className="store-actions">
        <button className="rebuild-index-button" onClick={tryRebuildIndex} title="Rebuild index">
        </button>
        {tags.slice(0, 6).map(tag => (
          <button key={tag} className="tag-button" onClick={() => console.log('clicked tag', tag)}>
            {tag}
          </button>
        ))}
        <select
          value={resultsSort}
          onChange={(e) => {
            setResultsSort(e.target.value);
            sortApps(e.target.value);
          }}
          className="sort-select"
        >
          <option>Recently published</option>
          <option>Most popular</option>
          <option>Best rating</option>
          <option>Recently updated</option>
        </select>
      </div>
      {!searchQuery && (
        <div className="featured-apps">
          <h2>Featured Apps</h2>
          <div className="featured-apps-grid">
            {listedApps.filter(app => featuredPackageNames.includes(app.package)).map((app) => (
              <AppEntry
                key={appId(app) + (app.state?.our_version || "")}
                size="medium"
                app={app}
                launchPath={launchPaths[app.package]}
                className="featured-app-entry"
              />
            ))}
          </div>
        </div>
      )}
      <h2>{searchQuery ? 'Search Results' : 'All Apps'}</h2>
      <div className="app-list">
        {displayedApps
          .filter(app => searchQuery ? true : !featuredPackageNames.includes(app.package))
          .map(app => (
            <AppEntry
              key={appId(app) + (app.state?.our_version || "")}
              size="large"
              app={app}
              className="app-entry"
              overrideImageSize="medium"
              showMoreActions={true}
            />
          ))}
      </div>
      {pages.length > 1 && (
        <div className="pagination">
          <button className="pagination-button" onClick={() => page !== pages[0] && setPage(page - 1)}>
            <FaChevronLeft />
          </button>
          {pages.map((p) => (
            <button
              key={`page-${p}`}
              className={`pagination-button ${p === page ? 'active' : ''}`}
              onClick={() => setPage(p)}
            >
              {p}
            </button>
          ))}
          <button className="pagination-button" onClick={() => page !== pages[pages.length - 1] && setPage(page + 1)}>
            <FaChevronRight />
          </button>
        </div>
      )}
      <Modal title="Rebuilding index..." show={isRebuildingIndex} hide={() => { }}>
        <Loader msg="This may take a few seconds." />
      </Modal>
    </div>
  );
}