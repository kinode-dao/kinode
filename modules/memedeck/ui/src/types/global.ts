declare module "global" {
  global {
    interface Window {
      our: {
        node: string;
        process: string;
      };
    }
  }
}
