import QtQuick
import ".."

// Original vector vocabulary on a 24-unit grid. No glyph font, asset download or theme dependency.
// The React chrome uses lucide icons; the names it needs (pyramid, cpu, sparkles, gear, box, briefcase, globe,
// shield, shield-check, book, git-branch, layers, chart, download, upload, circle, check-circle, star, arrow-right,
// more-v, trash, loader, mic, image, home, up, moon, sun, scroll, alert-circle, zap) are drawn from lucide's path
// data (stroke 2, round caps and joins) through a small SVG path interpreter. Every pre-fidelity name is preserved.
Canvas {
    id: icon
    property string name: ''
    property color color: Theme.textPrimary
    property real strokeWidth: 2
    property real size: 18
    implicitWidth: size
    implicitHeight: size
    onNameChanged: requestPaint()
    onColorChanged: requestPaint()
    onStrokeWidthChanged: requestPaint()
    onWidthChanged: requestPaint()
    onHeightChanged: requestPaint()
    onPaint: {
        var c = getContext('2d'); c.reset(); c.scale(width/24, height/24);
        c.strokeStyle = color; c.fillStyle = color; c.lineWidth = strokeWidth;
        c.lineCap = 'round'; c.lineJoin = 'round';
        function path(points, close) {
            c.beginPath(); c.moveTo(points[0][0], points[0][1]);
            for (var i=1; i<points.length; ++i) c.lineTo(points[i][0], points[i][1]);
            if (close) c.closePath(); c.stroke();
        }
        function box(x,y,w,h) { c.strokeRect(x,y,w,h); }
        function rr(x,y,w,h,r) { c.beginPath(); c.moveTo(x+r,y); c.lineTo(x+w-r,y); c.arc(x+w-r,y+r,r,-Math.PI/2,0); c.lineTo(x+w,y+h-r); c.arc(x+w-r,y+h-r,r,0,Math.PI/2); c.lineTo(x+r,y+h); c.arc(x+r,y+h-r,r,Math.PI/2,Math.PI); c.lineTo(x,y+r); c.arc(x+r,y+r,r,Math.PI,Math.PI*1.5); c.closePath(); c.stroke(); }
        function ring(x,y,r) { c.beginPath(); c.arc(x,y,r,0,Math.PI*2); c.stroke(); }
        function dot(x,y,r) { c.beginPath(); c.arc(x,y,r,0,Math.PI*2); c.fill(); }
        function arc(x,y,r,a0,a1) { c.beginPath(); c.arc(x,y,r,a0,a1); c.stroke(); }
        // SVG elliptical arc → canvas (endpoint to centre parameterisation, SVG implementation notes F.6.5).
        function svgArc(x1,y1,rx,ry,phi,fa,fs,x2,y2) {
            if (rx === 0 || ry === 0) { c.lineTo(x2,y2); return; }
            var rad = phi*Math.PI/180, cos = Math.cos(rad), sin = Math.sin(rad);
            var dx = (x1-x2)/2, dy = (y1-y2)/2, x1p = cos*dx + sin*dy, y1p = -sin*dx + cos*dy;
            rx = Math.abs(rx); ry = Math.abs(ry);
            var lam = (x1p*x1p)/(rx*rx) + (y1p*y1p)/(ry*ry);
            if (lam > 1) { rx *= Math.sqrt(lam); ry *= Math.sqrt(lam); }
            var num = rx*rx*ry*ry - rx*rx*y1p*y1p - ry*ry*x1p*x1p, den = rx*rx*y1p*y1p + ry*ry*x1p*x1p;
            var coef = (fa === fs ? -1 : 1) * Math.sqrt(Math.max(0, den ? num/den : 0));
            var cxp = coef*rx*y1p/ry, cyp = -coef*ry*x1p/rx;
            var cx = cos*cxp - sin*cyp + (x1+x2)/2, cy = sin*cxp + cos*cyp + (y1+y2)/2;
            function ang(ux,uy,vx,vy) { var d = ux*vx+uy*vy, l = Math.sqrt(ux*ux+uy*uy)*Math.sqrt(vx*vx+vy*vy); var a = Math.acos(Math.max(-1, Math.min(1, d/l))); return (ux*vy-uy*vx < 0) ? -a : a; }
            var th1 = ang(1,0,(x1p-cxp)/rx,(y1p-cyp)/ry), dth = ang((x1p-cxp)/rx,(y1p-cyp)/ry,(-x1p-cxp)/rx,(-y1p-cyp)/ry);
            if (!fs && dth > 0) dth -= 2*Math.PI; else if (fs && dth < 0) dth += 2*Math.PI;
            if (rx === ry && phi === 0) { c.arc(cx,cy,rx,th1,th1+dth,dth < 0); return; }
            var n = Math.max(2, Math.ceil(Math.abs(dth)/(Math.PI/8)));
            for (var i=1; i<=n; ++i) { var t = th1 + dth*i/n; c.lineTo(cos*rx*Math.cos(t) - sin*ry*Math.sin(t) + cx, sin*rx*Math.cos(t) + cos*ry*Math.sin(t) + cy); }
        }
        // Lucide path data (M L H V C S Q T A Z, absolute or relative), stroked.
        function d(data) {
            var tokens = data.match(/[MmLlHhVvCcSsQqTtAaZz]|[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?/g) || [];
            var i = 0, cmd = '', x = 0, y = 0, sx = 0, sy = 0, cpx = null, cpy = null, qx = null, qy = null;
            function num() { return parseFloat(tokens[i++]); }
            c.beginPath();
            while (i < tokens.length) {
                if (/[A-Za-z]/.test(tokens[i])) cmd = tokens[i++];
                else if (cmd === 'M') cmd = 'L'; else if (cmd === 'm') cmd = 'l';
                var rel = cmd === cmd.toLowerCase(), ox = rel ? x : 0, oy = rel ? y : 0, prev = cmd;
                switch (cmd.toUpperCase()) {
                case 'M': x = ox + num(); y = oy + num(); c.moveTo(x,y); sx = x; sy = y; break;
                case 'L': x = ox + num(); y = oy + num(); c.lineTo(x,y); break;
                case 'H': x = ox + num(); c.lineTo(x,y); break;
                case 'V': y = oy + num(); c.lineTo(x,y); break;
                case 'C': { var c1x = ox+num(), c1y = oy+num(), c2x = ox+num(), c2y = oy+num(); x = ox+num(); y = oy+num(); c.bezierCurveTo(c1x,c1y,c2x,c2y,x,y); cpx = c2x; cpy = c2y; break; }
                case 'S': { var r1x = cpx === null ? x : 2*x-cpx, r1y = cpy === null ? y : 2*y-cpy, s2x = ox+num(), s2y = oy+num(); x = ox+num(); y = oy+num(); c.bezierCurveTo(r1x,r1y,s2x,s2y,x,y); cpx = s2x; cpy = s2y; break; }
                case 'Q': { var q1x = ox+num(), q1y = oy+num(); x = ox+num(); y = oy+num(); c.quadraticCurveTo(q1x,q1y,x,y); qx = q1x; qy = q1y; break; }
                case 'T': { var t1x = qx === null ? x : 2*x-qx, t1y = qy === null ? y : 2*y-qy; x = ox+num(); y = oy+num(); c.quadraticCurveTo(t1x,t1y,x,y); qx = t1x; qy = t1y; break; }
                case 'A': { var rx = num(), ry = num(), phi = num(), fa = num(), fs = num(), ex = ox+num(), ey = oy+num(); svgArc(x,y,rx,ry,phi,fa,fs,ex,ey); x = ex; y = ey; break; }
                case 'Z': c.closePath(); x = sx; y = sy; break;
                default: i = tokens.length; break;
                }
                if (prev.toUpperCase() !== 'C' && prev.toUpperCase() !== 'S') { cpx = null; cpy = null; }
                if (prev.toUpperCase() !== 'Q' && prev.toUpperCase() !== 'T') { qx = null; qy = null; }
            }
            c.stroke();
        }
        switch(name.toLowerCase()) {
        case 'dashboard': rr(3,3,7,7,1); rr(14,3,7,7,1); rr(3,14,7,7,1); rr(14,14,7,7,1); break;
        case 'learning': path([[12,6],[8,4],[3,4],[3,19],[8,19],[12,21],[16,19],[21,19],[21,4],[16,4],[12,6],[12,21]]); break;
        case 'strategy': path([[12,2],[22,12],[12,22],[2,12]],true); path([[8,16],[10,10],[16,8],[14,14]],true); break;
        case 'goals': ring(12,12,9); ring(12,12,4); break;
        case 'tasks': rr(3,4,18,17,2); path([[7,12],[10,15],[17,8]]); break;
        case 'notes': d('M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z'); d('M14 2v4a2 2 0 0 0 2 2h4'); d('M10 9H8'); d('M16 13H8'); d('M16 17H8'); break;
        case 'file': d('M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z'); d('M14 2v4a2 2 0 0 0 2 2h4'); break;
        case 'portal': path([[3,7],[20,7],[16,3]]); path([[21,17],[4,17],[8,21]]); break;
        case 'settings': d('M21 4h-6'); d('M11 4H3'); d('M21 12h-8'); d('M9 12H3'); d('M21 20h-4'); d('M13 20H3'); d('M15 2v4'); d('M9 10v4'); d('M17 18v4'); break;
        case 'filter': d('M22 3H2l8 9.46V19l4 2v-8.54L22 3z'); break;
        case 'search': ring(11,11,8); d('m21 21-4.3-4.3'); break;
        case 'expand': d('M8 3H5a2 2 0 0 0-2 2v3'); d('M21 8V5a2 2 0 0 0-2-2h-3'); d('M3 16v3a2 2 0 0 0 2 2h3'); d('M16 21h3a2 2 0 0 0 2-2v-3'); break;
        case 'dock': d('M8 3v3a2 2 0 0 1-2 2H3'); d('M21 8h-3a2 2 0 0 1-2-2V3'); d('M3 16h3a2 2 0 0 1 2 2v3'); d('M16 21v-3a2 2 0 0 1 2-2h3'); break;
        case 'copy': rr(8,8,14,14,2); d('M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2'); break;
        case 'edit': d('M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z'); d('m15 5 4 4'); break;
        case 'list': d('M3 12h.01'); d('M3 18h.01'); d('M3 6h.01'); d('M8 12h13'); d('M8 18h13'); d('M8 6h13'); break;
        case 'board': rr(3,4,5,16,1); rr(10,4,5,11,1); rr(17,4,5,14,1); break;
        case 'close': case 'x': d('M18 6 6 18'); d('m6 6 12 12'); break;
        case 'left': d('m15 18-6-6 6-6'); break;
        case 'right': d('m9 18 6-6-6-6'); break;
        case 'down': d('m6 9 6 6 6-6'); break;
        case 'up': d('m18 15-6-6-6 6'); break;
        case 'check': d('M20 6 9 17l-5-5'); break;
        case 'select': rr(3,3,18,18,2); d('m9 12 2 2 4-4'); break;
        case 'play': d('M6 3L20 12 6 21 6 3z'); break;
        case 'pause': rr(14,4,4,16,1); rr(6,4,4,16,1); break;
        case 'console': rr(2,4,20,16,2); path([[6,8],[10,12],[6,16]]); path([[13,16],[18,16]]); break;
        case 'disk': case 'folder': d('M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z'); break;
        case 'warning': case 'alert': d('m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3'); d('M12 9v4'); d('M12 17h.01'); break;
        case 'alert-circle': ring(12,12,10); d('M12 8v4'); d('M12 16h.01'); break;
        case 'clock': ring(12,12,10); d('M12 6v6l4 2'); break;
        case 'more': dot(12,12,2); dot(19,12,2); dot(5,12,2); break;
        case 'more-v': dot(12,12,2); dot(12,5,2); dot(12,19,2); break;
        case 'plus': d('M5 12h14'); d('M12 5v14'); break;
        case 'source': d('m16 18 6-6-6-6'); d('m8 6-6 6 6 6'); break;
        // ---- lucide glyphs used by the React chrome ----
        case 'pyramid': d('M2.5 16.88a1 1 0 0 1-.32-1.43l9-13.02a1 1 0 0 1 1.64 0l9 13.01a1 1 0 0 1-.32 1.44A49.7 49.7 0 0 1 12 20a49.7 49.7 0 0 1-9.5-3.12z'); d('M12 2v18'); break;
        case 'cpu': rr(4,4,16,16,2); rr(9,9,6,6,1); d('M15 2v2'); d('M15 20v2'); d('M2 15h2'); d('M2 9h2'); d('M20 15h2'); d('M20 9h2'); d('M9 2v2'); d('M9 20v2'); break;
        case 'sparkles': d('M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z'); d('M20 3v4'); d('M22 5h-4'); d('M4 17v2'); d('M5 18H3'); break;
        case 'gear': d('M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z'); ring(12,12,3); break;
        case 'box': d('M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z'); d('m3.3 7 8.7 5 8.7-5'); d('M12 22V12'); break;
        case 'briefcase': d('M16 20V4a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16'); rr(2,6,20,14,2); break;
        case 'globe': ring(12,12,10); d('M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20'); d('M2 12h20'); break;
        case 'shield': d('M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z'); break;
        case 'shield-check': d('M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z'); d('m9 12 2 2 4-4'); break;
        case 'book': d('M12 7v14'); d('M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z'); break;
        case 'git-branch': d('M6 3v12'); ring(18,6,3); ring(6,18,3); d('M18 9a9 9 0 0 1-9 9'); break;
        case 'layers': d('M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z'); d('M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12'); d('M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17'); break;
        case 'chart': d('M3 3v16a2 2 0 0 0 2 2h16'); d('M18 17V9'); d('M13 17V5'); d('M8 17v-3'); break;
        case 'download': d('M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4'); d('m7 10 5 5 5-5'); d('M12 15V3'); break;
        case 'upload': d('M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4'); d('m17 8-5-5-5 5'); d('M12 3v12'); break;
        case 'circle': ring(12,12,10); break;
        case 'check-circle': ring(12,12,10); d('m9 12 2 2 4-4'); break;
        case 'star': d('M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z'); break;
        case 'arrow-right': d('M5 12h14'); d('m12 5 7 7-7 7'); break;
        case 'trash': d('M3 6h18'); d('M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6'); d('M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2'); d('M10 11v6'); d('M14 11v6'); break;
        case 'loader': { var spokes = ['M12 2v4','m16.2 7.8 2.9-2.9','M18 12h4','m16.2 16.2 2.9 2.9','M12 18v4','m4.9 19.1 2.9-2.9','M2 12h4','m4.9 4.9 2.9 2.9']; for (var j=0;j<8;++j){c.globalAlpha=0.3+0.7*j/7;d(spokes[j]);} c.globalAlpha=1; break; }
        case 'mic': d('M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z'); d('M19 10v2a7 7 0 0 1-14 0v-2'); d('M12 19v3'); break;
        case 'image': rr(3,3,18,18,2); ring(9,9,2); d('m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21'); break;
        case 'home': d('M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8'); d('M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z'); break;
        case 'moon': d('M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z'); break;
        case 'sun': ring(12,12,4); d('M12 2v2'); d('M12 20v2'); d('m4.93 4.93 1.41 1.41'); d('m17.66 17.66 1.41 1.41'); d('M2 12h2'); d('M20 12h2'); d('m6.34 17.66-1.41 1.41'); d('m19.07 4.93-1.41 1.41'); break;
        case 'scroll': d('M8 21h12a2 2 0 0 0 2-2v-2H10v2a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v3h4'); d('M19 17V5a2 2 0 0 0-2-2H4'); break;
        case 'zap': d('M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z'); break;
        default: ring(12,12,3);
        }
    }
    Accessible.ignored: true
}
