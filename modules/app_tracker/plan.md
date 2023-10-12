### App Tracker: our built-in package manager that lives in userspace

*note: 'app' and 'package' will be used interchangeably, but they are the same thing. generally, end users should see 'apps', and developers and the system itself should see 'packages'*

### Backend

Tracker requires full read-write to filesystem, along with caps for every other distro app and runtime module. It takes all the caps because it needs the ability to grant them to packages we install!

In order to load in the currently installed packages, Tracker will access the VFS and read from a hardcoded set of

### Frontend

Tracker will present a frontend that shows all the apps you currently have installed. You can see some metadata about them, and uninstall them from this list.

Tracker will contain a "store" to browse for new apps to install. TODO