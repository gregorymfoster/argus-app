import math
CREAM="#f5f1e8"; TERRA="#d97757"; TERRA_D="#b4532f"; INK="#141413"
def frame(defs, body, bg):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
<defs>
<filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="12" stdDeviation="14" flood-color="#000" flood-opacity="0.32"/></filter>
<clipPath id="sq"><rect x="100" y="100" width="824" height="824" rx="186"/></clipPath>
{bg[0]}
{defs}
</defs>
<rect x="100" y="100" width="824" height="824" rx="186" fill="{bg[1]}" filter="url(#sh)"/>
<g clip-path="url(#sq)">{body}
<rect x="100.5" y="100.5" width="823" height="823" rx="185.5" fill="none" stroke="#fff" stroke-opacity="0.10" stroke-width="2"/></g>
</svg>'''
DARKBG=('<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#3a3835"/><stop offset="1" stop-color="#1b1a18"/></linearGradient>','url(#bg)')
ALMOND="M 196 512 C 330 268, 694 268, 828 512 C 694 756, 330 756, 196 512 Z"

def aperture(cx,cy,R,r,n=6,rot=-90):
    V=[(cx+r*math.cos(math.radians(rot+360*i/n)),cy+r*math.sin(math.radians(rot+360*i/n))) for i in range(n)]
    def hit(a,b):  # extend a->b beyond b to circle
        dx,dy=b[0]-a[0],b[1]-a[1]; fx,fy=b[0]-cx,b[1]-cy
        A=dx*dx+dy*dy; B=2*(fx*dx+fy*dy); C=fx*fx+fy*fy-R*R
        t=(-B+math.sqrt(B*B-4*A*C))/(2*A); return (b[0]+t*dx,b[1]+t*dy)
    P=[hit(V[i],V[(i+1)%n]) for i in range(n)]
    blades=[]
    for i in range(n):
        a=V[(i+1)%n]; p0=P[i]; p1=P[(i+1)%n]
        blades.append(f"M {a[0]:.1f} {a[1]:.1f} L {p0[0]:.1f} {p0[1]:.1f} A {R} {R} 0 0 1 {p1[0]:.1f} {p1[1]:.1f} Z")
    hexa="M "+" L ".join(f"{x:.1f} {y:.1f}" for x,y in V)+" Z"
    return blades,hexa

# A: aperture eye
bl,hexa=aperture(512,512,186,66)
shades=["#e48a68","#d97757","#c9653f","#b4532f","#c9653f","#d97757"]
blades="".join(f'<path d="{d}" fill="{shades[i]}" stroke="#7a2f15" stroke-opacity="0.55" stroke-width="3" stroke-linejoin="round"/>' for i,d in enumerate(bl))
A=frame('''<linearGradient id="scl" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fbf8f1"/><stop offset="1" stop-color="#e6e0d3"/></linearGradient>
<clipPath id="eye"><path d="'''+ALMOND+'''"/></clipPath>
<radialGradient id="shade" cx="0.5" cy="0.5" r="0.5"><stop offset="0.75" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="0.35"/></radialGradient>''',
f'''<path d="{ALMOND}" fill="url(#scl)"/>
<g clip-path="url(#eye)">{blades}<path d="{hexa}" fill="{INK}"/><circle cx="512" cy="512" r="186" fill="url(#shade)"/>
<path d="M 196 512 C 330 268, 694 268, 828 512" fill="none" stroke="#000" stroke-opacity="0.18" stroke-width="22"/></g>
<circle cx="566" cy="452" r="26" fill="#fff" fill-opacity="0.92"/>''',DARKBG)

# B: peacock ocellus (Argus's eyes on the peacock's tail)
barbs="".join(f'<line x1="512" y1="560" x2="{512+470*math.cos(math.radians(a)):.0f}" y2="{560+470*math.sin(math.radians(a)):.0f}" stroke="{TERRA}" stroke-opacity="0.16" stroke-width="5"/>' for a in range(0,360,9))
def drop(s,cy=560):
    # teardrop pointing up, scale s
    return f"M 512 {cy-330*s:.0f} C {512+150*s:.0f} {cy-250*s:.0f}, {512+250*s:.0f} {cy-110*s:.0f}, {512+250*s:.0f} {cy+20*s:.0f} C {512+250*s:.0f} {cy+170*s:.0f}, {512+130*s:.0f} {cy+250*s:.0f}, 512 {cy+250*s:.0f} C {512-130*s:.0f} {cy+250*s:.0f}, {512-250*s:.0f} {cy+170*s:.0f}, {512-250*s:.0f} {cy+20*s:.0f} C {512-250*s:.0f} {cy-110*s:.0f}, {512-150*s:.0f} {cy-250*s:.0f}, 512 {cy-330*s:.0f} Z"
B=frame('''<radialGradient id="ter" cx="0.5" cy="0.55" r="0.6"><stop offset="0" stop-color="#e8906f"/><stop offset="1" stop-color="#b4532f"/></radialGradient>
<radialGradient id="heart" cx="0.45" cy="0.4" r="0.7"><stop offset="0" stop-color="#2b3a4a"/><stop offset="1" stop-color="#0f1419"/></radialGradient>''',
f'''{barbs}<path d="{drop(1.0)}" fill="url(#ter)"/><path d="{drop(0.72,575)}" fill="{CREAM}"/><path d="{drop(0.6,585)}" fill="#c9653f"/><ellipse cx="512" cy="625" rx="104" ry="112" fill="url(#heart)"/><circle cx="548" cy="590" r="20" fill="#fff" fill-opacity="0.9"/>''',DARKBG)

# C: hundred eyes iris (phyllotaxis of 100 dots)
dots=[]
g=math.radians(137.508)
for k in range(1,101):
    rr=78+ 120*math.sqrt(k/100); th=k*g
    t=k/100; size=7+13*t
    col="#f2c6ae" if t<0.3 else ("#e48a68" if t<0.65 else TERRA)
    dots.append(f'<circle cx="{512+rr*math.cos(th):.1f}" cy="{512+rr*math.sin(th):.1f}" r="{size:.1f}" fill="{col}"/>')
C=frame('<radialGradient id="glow" cx="0.5" cy="0.5" r="0.5"><stop offset="0" stop-color="#d97757" stop-opacity="0.35"/><stop offset="1" stop-color="#d97757" stop-opacity="0"/></radialGradient>',
f'''<circle cx="512" cy="512" r="330" fill="url(#glow)"/><circle cx="512" cy="512" r="226" fill="#2a1a14"/>{"".join(dots)}<circle cx="512" cy="512" r="70" fill="{INK}"/><circle cx="538" cy="486" r="18" fill="#fff" fill-opacity="0.92"/>''',DARKBG)

# D: light paper, bold terracotta almond with aperture pupil
bl2,hexa2=aperture(512,512,118,52)
LIGHT=('<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fdfcf8"/><stop offset="1" stop-color="#ece8dd"/></linearGradient>','url(#bg)')
D=frame('', f'''<path d="{ALMOND}" fill="{TERRA}"/><circle cx="512" cy="512" r="150" fill="#fdfcf8"/>'''+"".join(f'<path d="{d}" fill="{INK}" fill-opacity="{0.78+0.04*(i%3)}" stroke="#fdfcf8" stroke-width="7" stroke-linejoin="round"/>' for i,d in enumerate(bl2))+f'<circle cx="512" cy="512" r="50" fill="{TERRA}" fill-opacity="0"/>',LIGHT)

for n,s in zip("ABCD",[A,B,C,D]): open(f"{n}.svg","w").write(s)
import math
# E: Aperture (7 blades, full lens)
bl,hexa=aperture(512,512,300,92,n=7,rot=-90)
sh=["#e8906f","#d97757","#c9653f","#b4532f","#a3482a","#c05a36","#d46d4b"]
E=frame('<radialGradient id="vig" cx="0.5" cy="0.5" r="0.5"><stop offset="0.7" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="0.35"/></radialGradient><radialGradient id="hole" cx="0.42" cy="0.38" r="0.7"><stop offset="0" stop-color="#2a2826"/><stop offset="1" stop-color="#0d0d0c"/></radialGradient>',
 '<circle cx="512" cy="512" r="330" fill="#0f0e0d"/><circle cx="512" cy="512" r="322" fill="none" stroke="#f5f1e8" stroke-opacity="0.18" stroke-width="6"/>'+
 "".join(f'<path d="{d}" fill="{sh[i]}" stroke="#5e2410" stroke-opacity="0.6" stroke-width="4" stroke-linejoin="round"/>' for i,d in enumerate(bl))+
 f'<path d="{hexa}" fill="url(#hole)"/><circle cx="512" cy="512" r="300" fill="url(#vig)"/><path d="M 330 300 A 260 260 0 0 1 560 230" fill="none" stroke="#fff" stroke-opacity="0.35" stroke-width="14" stroke-linecap="round"/>',DARKBG)
# F: Peak (mountain that doubles as an A, with a sun)
F=frame('<linearGradient id="pk" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#e8906f"/><stop offset="1" stop-color="#b4532f"/></linearGradient><linearGradient id="pk2" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#7c3a24"/><stop offset="1" stop-color="#4a2316"/></linearGradient>',
 '<circle cx="690" cy="330" r="78" fill="#f5f1e8"/>'
 '<path d="M 100 760 L 300 470 Q 318 446 336 470 L 470 650 L 100 924 Z" fill="url(#pk2)"/>'
 '<path d="M 170 924 L 488 262 Q 512 222 536 262 L 854 924 Z" fill="url(#pk)"/>'
 '<path d="M 512 470 L 640 924 L 384 924 Z" fill="#1b1a18" fill-opacity="0.0"/>'
 '<path d="M 380 640 L 644 640" stroke="#f5f1e8" stroke-width="30" stroke-linecap="round"/>'
 '<path d="M 445 360 L 488 280 Q 512 240 536 280 L 579 360 L 545 340 L 512 372 L 479 340 Z" fill="#f5f1e8"/>',DARKBG)
# G: Stack (fanned photos, the top one picked)
def card(rot,fill,inner=""):
    return f'<g transform="rotate({rot} 512 540)"><rect x="262" y="300" width="500" height="440" rx="34" fill="{fill}" filter="url(#cs)"/>{inner}</g>'
photo='<clipPath id="ph"><rect x="292" y="330" width="440" height="330" rx="14"/></clipPath><g clip-path="url(#ph)"><rect x="292" y="330" width="440" height="330" fill="url(#sky)"/><circle cx="640" cy="420" r="40" fill="#fbe3d5"/><path d="M 292 660 L 430 470 L 520 590 L 590 520 L 732 660 Z" fill="#2a1a14"/></g>'
G=frame('<filter id="cs" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-opacity="0.4"/></filter><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#e8906f"/><stop offset="1" stop-color="#c6613f"/></linearGradient>',
 card(-14,"#6b6760")+card(-6,"#a9a499")+card(4,"#f5f1e8",photo),DARKBG)
# H: Dial (a hundred ticks, a terracotta arc)
ticks=[]
for k in range(100):
    a=math.radians(-90+3.6*k); on=k<34; r1=228 if k%5 else 214; r2=300
    ticks.append(f'<line x1="{512+r1*math.cos(a):.1f}" y1="{512+r1*math.sin(a):.1f}" x2="{512+r2*math.cos(a):.1f}" y2="{512+r2*math.sin(a):.1f}" stroke="{"#d97757" if on else "#f5f1e8"}" stroke-opacity="{1 if on else 0.42}" stroke-width="{14 if k%5==0 else 9}" stroke-linecap="round"/>')
H=frame('<radialGradient id="kn" cx="0.4" cy="0.35" r="0.8"><stop offset="0" stop-color="#3d3a36"/><stop offset="1" stop-color="#161514"/></radialGradient>',
 "".join(ticks)+'<circle cx="512" cy="512" r="170" fill="url(#kn)" stroke="#000" stroke-opacity="0.4" stroke-width="4"/><circle cx="512" cy="512" r="168" fill="none" stroke="#fff" stroke-opacity="0.08" stroke-width="3"/><circle cx="512" cy="390" r="22" fill="#d97757"/>',DARKBG)
for n,s in zip("EFGH",[E,F,G,H]): open(f"{n}.svg","w").write(s)
import math
T="#d97757"; TD="#b4532f"; CR="#f5f1e8"
TERRABG=('<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#e3876a"/><stop offset="1" stop-color="#b9532f"/></linearGradient>','url(#bg)')
LIGHTBG=('<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fdfcf8"/><stop offset="1" stop-color="#e9e4d8"/></linearGradient>','url(#bg)')
SH='<filter id="cs" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-opacity="0.38"/></filter>'
out={}
# S2: bold flat glyph: two offset squares, front one terracotta with cream peak
out["S2"]=frame(SH, f'<rect x="300" y="250" width="440" height="440" rx="70" fill="{CR}" fill-opacity="0.25"/><rect x="250" y="330" width="460" height="460" rx="70" fill="{T}" filter="url(#cs)"/><circle cx="600" cy="450" r="44" fill="{CR}"/><path d="M 250 790 L 250 690 L 400 520 L 500 630 L 560 570 L 710 720 L 710 790 Z" fill="{CR}" clip-path="url(#sq)"/>', DARKBG)
# fix S2 peak clipping to card corners
out["S2"]=out["S2"].replace('<path d="M 250 790','<clipPath id="c2"><rect x="250" y="330" width="460" height="460" rx="70"/></clipPath><path clip-path="url(#c2)" d="M 250 790').replace(' clip-path="url(#sq)"/>','/>',1)
# S3: contact sheet 3x3, one tile picked
tiles=""
for r in range(3):
    for c in range(3):
        x=232+c*196; y=232+r*196; pick=(r,c)==(1,1)
        tiles+=f'<rect x="{x}" y="{y}" width="168" height="168" rx="22" fill="{T if pick else CR}" fill-opacity="{1 if pick else 0.16+0.05*((r+c)%2)}" {"filter=\"url(#cs)\"" if pick else ""}/>'
tiles+=f'<path d="M 428 596 L 488 516 L 528 566 L 552 540 L 596 596 Z" fill="{CR}"/><circle cx="560" cy="476" r="16" fill="{CR}"/>'
out["S3"]=frame(SH,tiles,DARKBG)
# S4: fanned hand of five prints
fan=""
cols=["#5e5a54","#7d786f","#9f998d","#c7c1b4",CR]
for i,a in enumerate([-28,-14,0,14,28]):
    fan+=f'<g transform="rotate({a} 512 860)"><rect x="352" y="270" width="320" height="400" rx="26" fill="{cols[i] if i!=2 else CR}" filter="url(#cs)"/></g>'
fan=fan.replace(f'rotate(0 512 860)"><rect x="352" y="270" width="320" height="400" rx="26" fill="{CR}"',f'rotate(0 512 860)"><rect x="352" y="270" width="320" height="400" rx="26" fill="{CR}"')
# move centre card last (on top) with a photo
center=f'<g><rect x="352" y="250" width="320" height="410" rx="26" fill="{CR}" filter="url(#cs)"/><rect x="376" y="274" width="272" height="290" rx="10" fill="url(#sky)"/><path d="M 376 564 L 460 450 L 520 520 L 560 480 L 648 564 Z" fill="#2a1a14"/><circle cx="590" cy="330" r="26" fill="#fbe3d5"/></g>'
fan_parts=fan.split("</g>")[:-1]; fan_parts=[p+"</g>" for p in fan_parts]; fan="".join(fan_parts[:2]+fan_parts[3:])+center
out["S4"]=frame(SH+'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#e8906f"/><stop offset="1" stop-color="#c6613f"/></linearGradient>',fan,DARKBG)
# S5: light stack, ink + terracotta
def card(rot,fill,inner="",op=1):
    return f'<g transform="rotate({rot} 512 540)"><rect x="262" y="300" width="500" height="440" rx="34" fill="{fill}" fill-opacity="{op}" filter="url(#cs)"/>{inner}</g>'
photo='<clipPath id="ph"><rect x="292" y="330" width="440" height="330" rx="14"/></clipPath><g clip-path="url(#ph)"><rect x="292" y="330" width="440" height="330" fill="url(#sky)"/><circle cx="640" cy="420" r="40" fill="#fbe3d5"/><path d="M 292 660 L 430 470 L 520 590 L 590 520 L 732 660 Z" fill="#2a1a14"/></g>'
SKY='<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#e8906f"/><stop offset="1" stop-color="#c6613f"/></linearGradient>'
out["S5"]=frame(SH+SKY, card(-14,"#3a3835",op=0.35)+card(-6,"#3a3835",op=0.6)+card(4,"#262624",photo), LIGHTBG)
# S6: stack on terracotta ground, cream prints, ink photo
photo2=photo.replace('url(#sky)','#262624').replace('#fbe3d5',T).replace('#2a1a14',T)
out["S6"]=frame(SH, card(-14,CR,op=0.35)+card(-6,CR,op=0.6)+card(4,CR,photo2), TERRABG)

def ticks(n=100,on=34,r1=228,r1m=212,r2=300,wa=7,wb=11,oncol=T,offcol=CR,offop=0.42,start=-90,span=360):
    s=""
    for k in range(n):
        a=math.radians(start+span*k/n); maj=k%5==0; o=k<on
        s+=f'<line x1="{512+(r1m if maj else r1)*math.cos(a):.1f}" y1="{512+(r1m if maj else r1)*math.sin(a):.1f}" x2="{512+r2*math.cos(a):.1f}" y2="{512+r2*math.sin(a):.1f}" stroke="{oncol if o else offcol}" stroke-opacity="{1 if o else offop}" stroke-width="{wb if maj else wa}" stroke-linecap="round"/>'
    return s
KN='<radialGradient id="kn" cx="0.4" cy="0.35" r="0.8"><stop offset="0" stop-color="#3d3a36"/><stop offset="1" stop-color="#161514"/></radialGradient>'
knob='<circle cx="512" cy="512" r="170" fill="url(#kn)" stroke="#000" stroke-opacity="0.4" stroke-width="4"/><circle cx="512" cy="512" r="168" fill="none" stroke="#fff" stroke-opacity="0.08" stroke-width="3"/>'
out["D1"]=frame(KN, ticks()+knob+f'<circle cx="512" cy="390" r="22" fill="{T}"/>', DARKBG)
# D2: dial on terracotta, cream ticks, ink knob
out["D2"]=frame(KN, ticks(on=0,offcol=CR,offop=0.85)+knob+f'<circle cx="512" cy="390" r="22" fill="{T}"/>', TERRABG)
# D3: Lightroom-style 270 deg arc slider, no knob, big pointer dot
arc=ticks(n=76,on=46,r1=250,r1m=236,r2=300,start=135,span=270)
a=math.radians(135+270*46/76)
out["D3"]=frame('', arc+f'<circle cx="{512+190*math.cos(a):.0f}" cy="{512+190*math.sin(a):.0f}" r="34" fill="{T}"/><circle cx="512" cy="512" r="60" fill="{CR}" fill-opacity="0.12"/>', DARKBG)
# D4: 100 dots, cream to terracotta sweep
dots=""
for k in range(100):
    a=math.radians(-90+3.6*k); t=k/99
    col=f"rgb({int(245+(217-245)*t)},{int(241+(119-241)*t)},{int(232+(87-232)*t)})"
    dots+=f'<circle cx="{512+270*math.cos(a):.1f}" cy="{512+270*math.sin(a):.1f}" r="{6+8*t:.1f}" fill="{col}"/>'
out["D4"]=frame('', dots+f'<circle cx="512" cy="512" r="150" fill="{T}"/><circle cx="512" cy="512" r="150" fill="none" stroke="#000" stroke-opacity="0.2" stroke-width="6"/>', DARKBG)
# D5: light lens focus ring
out["D5"]=frame(KN, '<circle cx="512" cy="512" r="320" fill="#1f1e1c"/>'+ticks(on=0,r1=246,r1m=232,r2=296,offcol=CR,offop=0.7)+f'<circle cx="512" cy="512" r="200" fill="#121110"/><circle cx="512" cy="512" r="200" fill="none" stroke="{T}" stroke-width="16"/><path d="M 420 440 A 130 130 0 0 1 560 395" fill="none" stroke="#fff" stroke-opacity="0.35" stroke-width="14" stroke-linecap="round"/>', LIGHTBG)
# X1 hybrid: dial whose centre is a photo
X=('<clipPath id="pc"><circle cx="512" cy="512" r="180"/></clipPath>'+SKY)
out["X1"]=frame(X, ticks()+f'<g clip-path="url(#pc)"><rect x="300" y="300" width="424" height="424" fill="url(#sky)"/><circle cx="580" cy="440" r="34" fill="#fbe3d5"/><path d="M 300 724 L 300 640 L 450 500 L 540 590 L 600 540 L 724 650 L 724 724 Z" fill="#2a1a14"/></g><circle cx="512" cy="512" r="180" fill="none" stroke="#000" stroke-opacity="0.35" stroke-width="6"/>', DARKBG)
# X2 hybrid: stack with a 100-tick arc rising over it
out["X2"]=frame(SH+SKY, ticks(n=60,on=22,r1=322,r1m=308,r2=360,start=200,span=140,wa=6,wb=9)+card(-8,"#7d786f")+card(3,CR,photo), DARKBG)
for k,v in out.items(): open(f"{k}.svg","w").write(v)
print(" ".join(out))
