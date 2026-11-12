Class IadSm172Gen.C009 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property K3 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Property F3 As %Integer;

Index IDKEY On (K1, K2, K3) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Delimiter>"^"</Delimiter>
<Node>"d"</Node>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>"^"</Delimiter>
<Node>"d"</Node>
<Piece>2</Piece>
</Data>
<Data name="F3">
<RetrievalCode> s {*}={L1}</RetrievalCode>
</Data>
<Global>^IadSm172Gen009</Global>
<Subscript name="1">
<AccessType>sub</AccessType>
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="2">
<AccessType>sub</AccessType>
<Expression>{K2}</Expression>
</Subscript>
<Subscript name="3">
<AccessType>sub</AccessType>
<Expression>{K3}</Expression>
<NextCode> s {L3}=$o(^IadSm172Gen009({L1},{L2},{L3}))
 f  {
	q:{L3}=""
	q:{L3}=+{L3}
	s {L3}=$o(^IadSm172Gen009({L1},{L2},{L3}))
 }</NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen009S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
