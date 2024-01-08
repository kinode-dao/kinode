export const genFetchRoute = (route: string) => {
  return window.location.pathname.includes('/http-proxy/serve/') ?
    `/http-proxy/serve/${window.location.pathname.split('/')[3]}/${route}`:
    route
}
