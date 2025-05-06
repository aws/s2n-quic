// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded "><a href="index.html"><strong aria-hidden="true">1.</strong> Introduction</a></li><li class="chapter-item expanded affix "><li class="part-title">User Guide</li><li class="chapter-item expanded "><div><strong aria-hidden="true">2.</strong> Introduction</div></li><li class="chapter-item expanded "><a href="user-guide/installation.html"><strong aria-hidden="true">3.</strong> Installation</a></li><li class="chapter-item expanded "><a href="user-guide/debugging.html"><strong aria-hidden="true">4.</strong> Debugging</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="user-guide/debugging-tracelog.html"><strong aria-hidden="true">4.1.</strong> Tracing Logs</a></li><li class="chapter-item expanded "><a href="user-guide/debugging-pcap.html"><strong aria-hidden="true">4.2.</strong> Packet Capture</a></li><li class="chapter-item expanded "><a href="user-guide/debugging-gso.html"><strong aria-hidden="true">4.3.</strong> GSO and GRO</a></li></ol></li><li class="chapter-item expanded "><li class="part-title">Developer Guide</li><li class="chapter-item expanded "><a href="dev-guide.html"><strong aria-hidden="true">5.</strong> Introduction</a></li><li class="chapter-item expanded "><a href="dev-guide/setup.html"><strong aria-hidden="true">6.</strong> Setup</a></li><li class="chapter-item expanded "><a href="dev-guide/ci.html"><strong aria-hidden="true">7.</strong> Continuous Integration</a></li><li class="chapter-item expanded "><a href="dev-guide/kani.html"><strong aria-hidden="true">8.</strong> Kani</a></li><li class="chapter-item expanded affix "><li class="part-title">Examples</li><li class="chapter-item expanded "><a href="examples/async-client-hello-callback.html"><strong aria-hidden="true">9.</strong> Async client hello callback</a></li><li class="chapter-item expanded "><a href="examples/custom-congestion-controller.html"><strong aria-hidden="true">10.</strong> Custom congestion controller</a></li><li class="chapter-item expanded "><div><strong aria-hidden="true">11.</strong> dos mitigation</div></li><li class="chapter-item expanded "><div><strong aria-hidden="true">12.</strong> Echo</div></li><li class="chapter-item expanded "><div><strong aria-hidden="true">13.</strong> Event framework</div></li><li class="chapter-item expanded "><div><strong aria-hidden="true">14.</strong> Jumbo frame</div></li><li class="chapter-item expanded "><div><strong aria-hidden="true">15.</strong> rustls mtls</div></li><li class="chapter-item expanded "><div><strong aria-hidden="true">16.</strong> rustls provider</div></li><li class="chapter-item expanded "><div><strong aria-hidden="true">17.</strong> turmoil provider</div></li><li class="chapter-item expanded "><div><strong aria-hidden="true">18.</strong> Unreliable datagram</div></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0].split("?")[0];
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
