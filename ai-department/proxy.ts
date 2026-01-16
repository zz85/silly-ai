const LM_STUDIO = "http://localhost:1234";
const html = await Bun.file("index.html").text();

Bun.serve({
  port: 3000,
  async fetch(req) {
    const url = new URL(req.url);
    if (url.pathname === "/" || url.pathname === "/index.html") {
      return new Response(html, { headers: { "Content-Type": "text/html" } });
    }
    if (req.method === "OPTIONS") {
      return new Response(null, { headers: corsHeaders() });
    }
    const res = await fetch(LM_STUDIO + url.pathname, {
      method: req.method,
      headers: { "Content-Type": "application/json" },
      body: req.body,
    });
    return new Response(res.body, { headers: { ...Object.fromEntries(res.headers), ...corsHeaders() } });
  },
});

function corsHeaders() {
  return {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
  };
}

console.log("Proxy running on http://localhost:3000");
