# Put the emboldened outlines (fontforge changeWeight, cubic) into the
# original TTF via cu2qu; keep all other tables; drop TT hinting programs.
import json
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables import ttProgram
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.areaPen import AreaPen
from fontTools.pens.recordingPen import RecordingPen, DecomposingRecordingPen

def draw(cs, pen):
    for c in cs:
        if not c: continue
        k=next(i for i,p in enumerate(c) if p[2]); c=c[k:]+c[:k]
        pen.moveTo(c[0][:2]); i=1; n=len(c)
        while i<=n:
            p=c[i%n]
            if p[2]: pen.lineTo(p[:2]); i+=1
            else:
                a,b,e=c[i%n],c[(i+1)%n],c[(i+2)%n]; pen.curveTo(a[:2],b[:2],e[:2]); i+=3
        pen.closePath()

def area(cs):
    ap=AreaPen(); draw(cs,ap); return ap.value

for n in (1,2,3):
    base=TTFont('fonts/AbletonSans-Light.ttf'); E=json.load(open(f'out/e{n}.json'))
    glyf=base['glyf']; order=base.getGlyphOrder(); gs=base.getGlyphSet()
    ap=AreaPen(gs); gs['o'].draw(ap); rev = (ap.value>0) != (area(E['o'])>0)
    done=0
    for name in order:
        g=glyf[name]
        if name in E and not g.isComposite() and g.numberOfContours>0:
            pen=TTGlyphPen(None); draw(E[name], Cu2QuPen(pen, max_err=0.5, reverse_direction=rev))
            glyf[name]=pen.glyph(); done+=1
    for t in ('fpgm','prep','cvt ','DSIG','hdmx','LTSH','VDMX'):
        if t in base: del base[t]
    base['gasp'].gaspRange={0xFFFF: 0x000A}
    base['maxp'].maxSizeOfInstructions=0
    for name in order:
        g=glyf[name]
        p=ttProgram.Program(); p.fromBytecode(b''); g.program=p
        g.recalcBounds(glyf)
        adv,_=base['hmtx'][name]; base['hmtx'][name]=(adv, getattr(g,'xMin',0))
    for rec in base['name'].names:
        if rec.nameID==4: rec.string=f'Ableton Sans Light +{n}'
        if rec.nameID==6: rec.string=f'AbletonSans-LightPlus{n}'
    base.save(f'out/AbletonSans-Light{n}.ttf'); print(n, 'rev', rev, done, len(order))
