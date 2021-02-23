/* globals document, window, console, URLSearchParams, XMLHttpRequest, $, history */

(function() {
    "use strict";
    const map = { client: {}, server: {}, test: {} };
    const color_type = { succeeded: "success", unsupported: "secondary disabled", failed: "danger"};

    // see https://stackoverflow.com/a/43466724/
    function formatTime(seconds) {
        return [
            parseInt(seconds / 60 / 60),
            parseInt(seconds / 60 % 60),
            parseInt(seconds % 60)
        ].join(":").replace(/\b(\d)\b/g, "0$1");
    }

    function getLogLink(log_dir, s2n_quic_log_dir, server, client, test, text, res) {
        var ttip = "<b>Test:</b> " + test + "<br>" +
            "<b>Client:</b> " + client + "<br>" +
            "<b>Server:</b> " + server + "<br>" +
            "<b>Result: <span class=\"text-" + color_type[res] + "\">" + res + "</span></b>";
        var log_url_pattern = s2n_quic_log_dir[server];

        if (!log_url_pattern) {
            log_url_pattern = s2n_quic_log_dir[client]
        }

        if (!log_url_pattern) {
            log_url_pattern = log_dir;
        }

        var log_url = log_url_pattern.replace("SERVER", server).replace("CLIENT", client).replace("TEST", test);

        var a = document.createElement("a");
        a.className = "btn btn-xs btn-" + color_type[res] + " " + res + " test-" + text.toLowerCase();
        var ttip_target = a;
        if (res !== "unsupported") {
            a.href = log_url;
            a.target = "_blank";
            ttip += "<br><br>(Click for logs.)";
        } else {
            var s = document.createElement("span");
            s.className = "d-inline-block";
            s.tabIndex = 0;
            a.style = "pointer-events: none;";
            s.appendChild(a);
            ttip_target = s;
        }
        ttip_target.title = ttip;
        $(ttip_target).attr("data-toggle", "tooltip").attr("data-placement", "bottom").attr("data-html", true).tooltip();
        $(ttip_target).click(function() { $(this).blur(); });
        a.appendChild(document.createTextNode(text));
        return ttip_target;
    }

    function makeClickable(e, url) {
        e.title = url;
        $(e).attr("role", "button").attr("data-href", url).attr("data-toggle", "tooltip").tooltip();
        e.onclick = function(e) { window.open(e.target.getAttribute("data-href")); };
    }

    function makeColumnHeaders(t, result) {
        for(var i = 0; i < result.servers.length; i++)
            t.appendChild(document.createElement("colgroup"));
        var thead = t.createTHead();
        var row = thead.insertRow(0);
        var cell = document.createElement("th");
        row.appendChild(cell);
        cell.scope = "col";
        cell.className = "table-light client-any";
        for(var i = 0; i < result.servers.length; i++) {
            cell = document.createElement("th");
            row.appendChild(cell);
            cell.scope = "col";
            cell.className = "table-light server-" + result.servers[i];
            if (result.hasOwnProperty("urls"))
                makeClickable(cell, result.urls[result.servers[i]]);
            cell.innerHTML = result.servers[i];
        }
    }

    function makeRowHeader(tbody, result, i) {
        var row = tbody.insertRow(i);
        var cell = document.createElement("th");
        cell.scope = "row";
        cell.className = "table-light client-" + result.clients[i];
        if (result.hasOwnProperty("urls"))
            makeClickable(cell, result.urls[result.clients[i]]);
        cell.innerHTML = result.clients[i];
        row.appendChild(cell);
        return row;
    }

    function fillInteropTable(result) {
        var index = 0;
        var appendResult = function(el, res, i, j) {
            result.results[index].forEach(function(item) {
                if(item.result !== res) return;
                el.appendChild(getLogLink(result.log_dir, result.s2n_quic_log_dir, result.servers[j], result.clients[i], item.name, item.abbr, res));
            });
        };

        var t = document.getElementById("interop");
        t.innerHTML = "";
        makeColumnHeaders(t, result);
        var tbody = t.createTBody();
        for(var i = 0; i < result.clients.length; i++) {
            var row = makeRowHeader(tbody, result, i);
            for(var j = 0; j < result.servers.length; j++) {
                var cell = row.insertCell(j+1);
                cell.className = "server-" + result.servers[j] + " client-" + result.clients[i];
                appendResult(cell, "succeeded", i, j);
                appendResult(cell, "unsupported", i, j);
                appendResult(cell, "failed", i, j);
                index++;
            }
        }
    }

    function fillMeasurementTable(result) {
        var t = document.getElementById("measurements");
        t.innerHTML = "";
        makeColumnHeaders(t, result);
        var tbody = t.createTBody();
        var index = 0;
        for(var i = 0; i < result.clients.length; i++) {
            var row = makeRowHeader(tbody, result, i);
            for(var j = 0; j < result.servers.length; j++) {
                var res = result.measurements[index];
                var cell = row.insertCell(j+1);
                cell.className = "server-" + result.servers[j] + " client-" + result.clients[i];
                for(var k = 0; k < res.length; k++) {
                    var measurement = res[k];
                    var link = getLogLink(result.log_dir, result.s2n_quic_log_dir, result.servers[j], result.clients[i], measurement.name, measurement.abbr, measurement.result);
                    if (measurement.result === "succeeded")
                        link.innerHTML += ": " + measurement.details;
                    cell.appendChild(link);
                }
                index++;
            }
        }
    }

    function dateToString(date) {
        return date.toLocaleDateString("en-US",  { timeZone: 'UTC' }) + " " + date.toLocaleTimeString("en-US", { timeZone: 'UTC', timeZoneName: 'short' });
    }

    function makeButton(type, text, tooltip) {
        var b = document.createElement("button");
        b.innerHTML = text;
        b.id = type + "-" + text.toLowerCase();
        if (tooltip) {
            b.title = tooltip;
            $(b).attr("data-toggle", "tooltip").attr("data-placement", "bottom").attr("data-html", true).tooltip();
        }
        b.type = "button";
        b.className = type + " btn btn-light";
        $(b).click(clickButton);
        return b;
    }

    function toggleHighlight(e) {
        const comp = e.target.id.split("-");
        const which = "." + comp[0] + "-" + comp[1] + "." + comp[2];
        $(which).toggleClass("btn-highlight");
    }

    function setButtonState() {
        var params = new URLSearchParams(history.state ? history.state.path : window.location.search);
        var show = {};
        Object.keys(map).forEach(type => {
            map[type] = params.getAll(type).map(x => x.toLowerCase().split(",")).flat();
            if (map[type].length === 0)
                map[type] = $("#" + type + " :button").get().map(x => x.id.replace(type + "-", ""));
            $("#" + type + " :button").removeClass("active font-weight-bold").addClass("text-muted font-weight-light").filter((i, e) => map[type].includes(e.id.replace(type + "-", ""))).addClass("active font-weight-bold").removeClass("text-muted font-weight-light");
            show[type] = map[type].map(e => "." + type + "-" + e);
        });

        $(".result td").add(".result th").add(".result td a").hide();

        const show_classes = show.client.map(el1 => show.server.map(el2 => el1 + el2)).flat().join();
        $(".client-any," + show_classes).show();

        $(".result " + show.client.map(e => "th" + e).join()).show();
        $(".result " + show.server.map(e => "th" + e).join()).show();
        $(".measurement," + show.test.join()).show();

        $("#test :button").each((i, e) => {
            $(e).find("span,br").remove();
            var count = { succeeded: 0, unsupported: 0, failed: 0};
            Object.keys(count).map(c => count[c] = $(".btn." + e.id + "." + c + ":visible").length);
            Object.keys(count).map(c => {
                e.appendChild(document.createElement("br"));
                var b = document.createElement("span");
                b.innerHTML = count[c];
                b.className = "btn btn-xs btn-" + color_type[c];
                if (e.classList.contains("active") === false)
                    b.className += " disabled";
                b.id = e.id + "-" + c;
                $(b).hover(toggleHighlight, toggleHighlight);
                e.appendChild(b);
            });
        });
    }

    function clickButton(e) {
        function toggle(array, value) {
            var index = array.indexOf(value);
            if (index === -1)
                array.push(value);
            else
                array.splice(index, 1);
        }

        var b = $(e.target).closest(":button")[0];
        b.blur();
        const type = [...b.classList].filter(x => Object.keys(map).includes(x))[0];
        const which = b.id.replace(type + "-", "");

        var params = new URLSearchParams(history.state ? history.state.path : window.location.search);
        if (params.has(type) && params.get(type))
            map[type] = params.get(type).split(",");
        else
            map[type] = $("#" + type + " :button").get().map(e => e.id.replace(type + "-", ""));

        toggle(map[type], which);
        params.set(type, map[type]);
        if (map[type].length === $("#" + type + " :button").length)
            params.delete(type);

        const comp = decodeURIComponent(params.toString());
        var refresh = window.location.protocol + "//" + window.location.host + window.location.pathname + (comp ? "?" + comp : "");
        window.history.pushState(null, null, refresh);

        setButtonState();
        return false;
    }

    function makeTooltip(name, desc) {
        return "<strong>" + name + "</strong>" + (desc === undefined ? "" : "<br>" + desc);
    }

    function process(result) {
        var startTime = new Date(1000*result.start_time);
        var endTime = new Date(1000*result.end_time);
        var duration = result.end_time - result.start_time;
        document.getElementById("lastrun-end").innerHTML = dateToString(endTime);
        document.getElementById("quic-vers").innerHTML =
            "<tt>" + result.quic_version + "</tt> (\"draft-" + result.quic_draft + "\")";

        fillInteropTable(result);
        fillMeasurementTable(result);

        $("#client").add("#server").add("#test").empty();
        $("#client").append(result.clients.map(e => makeButton("client", e)));
        $("#server").append(result.servers.map(e => makeButton("server", e)));
        if (result.hasOwnProperty("tests"))
            $("#test").append(Object.keys(result.tests).map(e => makeButton("test", e, makeTooltip(result.tests[e].name, result.tests[e].desc))));
        else {
            // TODO: this else can eventually be removed, when all past runs have the test descriptions in the json
            const tcases = result.results.concat(result.measurements).flat().map(x => [x.abbr, x.name]).filter((e, i, a) => a.map(x => x[0]).indexOf(e[0]) === i);
            $("#test").append(tcases.map(e => makeButton("test", e[0], makeTooltip(e[1]))));
        }
        setButtonState();

        $("table.result").delegate("td", "mouseover mouseleave", function(e) {
            const t = $(this).closest("table.result");
            if (e.type === "mouseover") {
                $(this).parent().addClass("hover-xy");
                t.children("colgroup").eq($(this).index()).addClass("hover-xy");
                t.find("th").eq($(this).index()).addClass("hover-xy");
            } else {
                $(this).parent().removeClass("hover-xy");
                t.children("colgroup").eq($(this).index()).removeClass("hover-xy");
                t.find("th").eq($(this).index()).removeClass("hover-xy");
            }
        });
    }

    function load(dir) {
        document.getElementsByTagName("body")[0].classList.add("loading");
        var xhr = new XMLHttpRequest();
        xhr.responseType = 'json';
        xhr.open('GET', 'logs/' + dir + '/result.json');
        xhr.onreadystatechange = function() {
            if(xhr.readyState !== XMLHttpRequest.DONE) return;
            if(xhr.status !== 200) {
                console.log("Received status: ", xhr.status);
                return;
            }
            process(xhr.response);
            document.getElementsByTagName("body")[0].classList.remove("loading");
        };
        xhr.send();
    }

    load("latest");
})();