std.escapeStringXml("<div class=\"foo\">hello & world</div>") == "&lt;div class=&quot;foo&quot;&gt;hello &amp; world&lt;/div&gt;" &&
std.escapeStringXml("no special chars") == "no special chars" &&
std.escapeStringXml("it's a 'test'") == "it&apos;s a &apos;test&apos;"
