import fontforge,sys,json
w=int(sys.argv[1]); out={}
f=fontforge.open('fonts/AbletonSans-Light.ttf')
f.is_quadratic=False
def ok(g): return all(abs(p.x)<3000 and abs(p.y)<3000 for c in g.foreground for p in c)
fix=[]
for g in f.glyphs():
    if g.foreground and not g.references:
        adv=g.width; orig=g.foreground.dup()
        g.changeWeight(w,"LCG",0,0,"squish")
        if not ok(g):
            # fallback: union of the outline and its stroked path (+w/2 per side)
            g.foreground=orig; o2=orig.dup()
            g.stroke("circular", w, "round", "round"); l=g.foreground; l+=o2; g.foreground=l
            g.removeOverlap(); g.addExtrema(); g.round()
            fix.append((g.glyphname, ok(g)))
        g.width=adv
        out[g.glyphname]=[[(p.x,p.y,bool(p.on_curve)) for p in c] for c in g.foreground]
json.dump(out,open(sys.argv[2],'w')); print('FIX',fix)
