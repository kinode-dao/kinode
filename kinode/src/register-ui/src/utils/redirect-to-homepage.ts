export const redirectToHomepage = () => {
    const interval = setInterval(async () => {
        const res = await fetch("/version", { credentials: 'include' });
        if (res.status == 200) {
            clearInterval(interval);
            window.location.replace("/");
        }
    }, 500);
};