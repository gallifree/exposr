export default function Head() {
  return (
    <>
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <link
        rel="icon"
        href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%236366f1' stroke-width='2.2' stroke-linecap='round'><path d='M3 12h4l3-7 4 14 3-7h4'/></svg>"
      />
      {/* Apply the saved theme before first paint to avoid a flash. */}
      <script
        dangerouslySetInnerHTML={{
          __html: `try{var t=localStorage.getItem("theme");if(t)document.documentElement.dataset.theme=t}catch(e){}`,
        }}
      />
    </>
  );
}
