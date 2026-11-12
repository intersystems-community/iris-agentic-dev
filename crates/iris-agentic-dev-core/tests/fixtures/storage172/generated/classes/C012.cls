Class IadSm172Gen.C012 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property K3 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On (K1, K2, K3) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Piece>1</Piece>
</Data>
<Data name="F2">
<Piece>2</Piece>
</Data>
<Global>^IadSm172Gen012</Global>
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
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen012S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
