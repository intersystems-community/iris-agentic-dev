/// A table over a bitmap-chunk global: the third subscript is the row ID found by code, and the
/// data node under it holds a $BIT string, so the global has no node at a third subscript.
Class IadSm172.Bits Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Word As %Integer;

Index IdKeyIdx On Word [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Chunks">
<Global>^IadSm172Bits</Global>
<Subscript name="1">
<Expression>0</Expression>
</Subscript>
<Subscript name="2">
<Accessvar name="2">
<Code>s {2D2}=({L2}-1)*64000-1</Code>
<Variable>2D2</Variable>
</Accessvar>
<Expression>{Word}\64000+1</Expression>
</Subscript>
<Subscript name="3">
<AccessType>Other</AccessType>
<Expression>{Word}</Expression>
<Invalidcondition name="1">
<Expression>'$bit({3D},{L3}-{2D2})</Expression>
</Invalidcondition>
<NextCode>
<![CDATA[ n pos s pos={L3}-{2D2}
 s:pos<0 pos=0
 s pos=$bitfind({3D},1,pos+1)
 i 'pos s {L3}="" QUIT
 s {L3}={2D2}+pos
 q]]></NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172BitsS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
