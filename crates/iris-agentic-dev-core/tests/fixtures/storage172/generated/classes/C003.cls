Class IadSm172Gen.C003 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On K1 [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Delimiter>$C(1)</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>$C(1)</Delimiter>
<Node>"d"</Node>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Gen003</Global>
<Subscript name="1">
<AccessType>sub</AccessType>
<Expression>{K1}</Expression>
<NextCode> s {L1}=$o(^IadSm172Gen003({L1}))
 f  {
	q:{L1}=""
	q:{L1}=+{L1}
	s {L1}=$o(^IadSm172Gen003({L1}))
 }</NextCode>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen003S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
